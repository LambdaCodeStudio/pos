//! Tests de la transacción crítica: confirmación de recepción.
//! Atómica, con lock, idempotente; aplica ledger de precios, proyecciones,
//! ledger de inventario, lotes y proyección de stock — todo o nada.

mod comun;

use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use comun::{app, crear_usuario_con_rol, pedir, token_para, ROL_ADMINISTRADOR_ID};

struct Escenario {
    app: axum::Router,
    token: String,
    producto_comun: Uuid,
    producto_con_vencimiento: Uuid,
    recepcion_id: Uuid,
}

/// Arma vía API: categoría, dos productos (uno controla vencimiento),
/// proveedor con precios sin IVA y una recepción en borrador.
async fn armar_escenario(pool: &PgPool) -> Escenario {
    let admin = crear_usuario_con_rol(pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (st, cat) = pedir(
        &app,
        "POST",
        "/catalogo/categorias",
        Some(&token),
        Some(json!({ "nombre": "Almacén", "markup_pct": "40.00", "iva_pct": "21.00" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "crear categoría: {cat}");
    let categoria_id = cat["id"].as_str().unwrap().to_string();

    let (st, prod) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Yerba 1kg",
            "categoria_id": categoria_id,
            "codigos_barras": ["7790000000001"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "crear producto: {prod}");
    let producto_comun: Uuid = prod["id"].as_str().unwrap().parse().unwrap();

    let (st, prod) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Leche entera 1L",
            "categoria_id": categoria_id,
            "controla_vencimiento": true,
            "iva_pct_override": "10.50",
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "crear producto con vencimiento: {prod}");
    let producto_con_vencimiento: Uuid = prod["id"].as_str().unwrap().parse().unwrap();

    let (st, prov) = pedir(
        &app,
        "POST",
        "/compras/proveedores",
        Some(&token),
        Some(json!({ "nombre": "Distribuidora Sur", "precios_con_iva": false })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "crear proveedor: {prov}");
    let proveedor_id = prov["id"].as_str().unwrap().to_string();

    let (st, rec) = pedir(
        &app,
        "POST",
        "/compras/recepciones",
        Some(&token),
        Some(json!({ "proveedor_id": proveedor_id })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "crear recepción: {rec}");
    let recepcion_id: Uuid = rec["id"].as_str().unwrap().parse().unwrap();

    Escenario {
        app,
        token,
        producto_comun,
        producto_con_vencimiento,
        recepcion_id,
    }
}

async fn cargar_items_estandar(esc: &Escenario) {
    // Yerba: 10 unidades a $10,00 sin IVA (hereda IVA 21 y markup 40 de la
    // categoría) → precio final 1000 × 1.21 × 1.40 = 1694.
    let (st, item) = pedir(
        &esc.app,
        "PUT",
        &format!("/compras/recepciones/{}/items", esc.recepcion_id),
        Some(&esc.token),
        Some(json!({
            "producto_id": esc.producto_comun,
            "cantidad": "10",
            "costo_centavos": 1000,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "cargar yerba: {item}");
    assert_eq!(item["precio_final_centavos"], 1694, "cascada + cálculo: {item}");

    // Leche: 24 unidades a $8,00 sin IVA, IVA override 10.5%, vencimiento
    // obligatorio → 800 × 1.105 × 1.40 = 1237.6 → 1238.
    let (st, item) = pedir(
        &esc.app,
        "PUT",
        &format!("/compras/recepciones/{}/items", esc.recepcion_id),
        Some(&esc.token),
        Some(json!({
            "producto_id": esc.producto_con_vencimiento,
            "cantidad": "24",
            "costo_centavos": 800,
            "vencimiento": "2026-08-15",
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "cargar leche: {item}");
    assert_eq!(item["precio_final_centavos"], 1238, "redondeo al final: {item}");
}

#[sqlx::test(migrations = "./migrations")]
async fn confirmar_aplica_precios_stock_y_lotes(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    cargar_items_estandar(&esc).await;

    let (st, resp) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{}/confirmar", esc.recepcion_id),
        Some(&esc.token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "confirmar: {resp}");
    assert_eq!(resp["estado"], "confirmada");

    // Proyecciones de precio y costo (costo normalizado CON IVA).
    let yerba = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
        "SELECT precio_actual_centavos, costo_actual_centavos FROM catalogo.productos WHERE id = $1",
    )
    .bind(esc.producto_comun)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(yerba.0, Some(1694));
    assert_eq!(yerba.1, Some(1210), "costo 1000 sin IVA normalizado a 1210 con IVA");

    // Ledger de precios: una entrada por ítem, referenciando la recepción.
    let entradas_historial = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM catalogo.precios_historial WHERE recepcion_id = $1",
    )
    .bind(esc.recepcion_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(entradas_historial, 2);

    // Ledger de inventario: el stock es literalmente SUM(cantidad).
    let stock_yerba = sqlx::query_scalar::<_, Decimal>(
        "SELECT COALESCE(SUM(cantidad), 0) FROM inventario.movimientos_stock WHERE producto_id = $1",
    )
    .bind(esc.producto_comun)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stock_yerba, Decimal::from(10));

    // Proyección stock_actual coincide con el ledger.
    let proyeccion = sqlx::query_scalar::<_, Decimal>(
        "SELECT cantidad FROM inventario.stock_actual WHERE producto_id = $1",
    )
    .bind(esc.producto_con_vencimiento)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(proyeccion, Decimal::from(24));

    // Lote creado SOLO para el producto que controla vencimiento, con el
    // movimiento asociado.
    let lotes = sqlx::query_as::<_, (Uuid, Decimal)>(
        "SELECT producto_id, cantidad_actual FROM inventario.lotes",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(lotes.len(), 1);
    assert_eq!(lotes[0].0, esc.producto_con_vencimiento);
    assert_eq!(lotes[0].1, Decimal::from(24));

    let movimientos_con_lote = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM inventario.movimientos_stock WHERE producto_id = $1 AND lote_id IS NOT NULL",
    )
    .bind(esc.producto_con_vencimiento)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(movimientos_con_lote, 1);

    // Los ítems quedan pendientes de etiquetado (el flujo de marcarlos vive
    // en `crate::etiquetado`, bajo el contrato HMAC del dispositivo — ver
    // tests/etiquetado.rs).
    let pendientes = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM compras.recepcion_items WHERE recepcion_id = $1 AND NOT etiquetado",
    )
    .bind(esc.recepcion_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pendientes, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn confirmar_recepcion_reindexa_el_fiado_pendiente_del_producto(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    cargar_items_estandar(&esc).await;
    let (st, _) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{}/confirmar", esc.recepcion_id),
        Some(&esc.token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Cliente fía 5 unidades de yerba al precio recién fijado ($16,94/u).
    let (st, cliente) = pedir(&esc.app, "POST", "/clientes", Some(&esc.token),
        Some(json!({ "nombre": "Doña Rosa" }))).await;
    assert_eq!(st, StatusCode::OK, "{cliente}");
    let cliente_id: Uuid = cliente["id"].as_str().unwrap().parse().unwrap();

    let (st, sesion) = pedir(&esc.app, "POST", "/ventas/sesiones", Some(&esc.token),
        Some(json!({ "monto_inicial_centavos": 0 }))).await;
    assert_eq!(st, StatusCode::OK, "{sesion}");
    let sesion_id: Uuid = sesion["id"].as_str().unwrap().parse().unwrap();

    let venta = json!({
        "id": Uuid::now_v7(),
        "sesion_id": sesion_id,
        "cliente_id": cliente_id,
        "total_centavos": 8470,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_comun,
            "precio_unitario_centavos": 1694,
            "cantidad": "5",
            "subtotal_centavos": 8470,
        }],
        "pagos": [{ "medio": "cuenta_corriente", "monto_centavos": 8470 }],
    });
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(venta)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    async fn saldo(pool: &PgPool, cliente_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT saldo_actual_centavos FROM clientes.clientes WHERE id = $1")
            .bind(cliente_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }
    assert_eq!(saldo(&pool, cliente_id).await, 8470);

    // Nueva recepción del mismo producto con otro costo (con IVA incluido
    // por default): el precio final pasa de $16,94 a $28,00 (2000 × 1.40).
    let (st, rec2) = pedir(&esc.app, "POST", "/compras/recepciones", Some(&esc.token),
        Some(json!({ "proveedor_id": null }))).await;
    // Recepción sin proveedor es válida (proveedor_id es opcional).
    assert_eq!(st, StatusCode::OK, "{rec2}");
    let recepcion2_id: Uuid = rec2["id"].as_str().unwrap().parse().unwrap();

    let (st, item) = pedir(&esc.app, "PUT", &format!("/compras/recepciones/{recepcion2_id}/items"),
        Some(&esc.token), Some(json!({
            "producto_id": esc.producto_comun,
            "cantidad": "10",
            "costo_centavos": 2000,
        }))).await;
    assert_eq!(st, StatusCode::OK, "{item}");
    assert_eq!(item["precio_final_centavos"], 2800);

    let (st, resp) = pedir(&esc.app, "POST", &format!("/compras/recepciones/{recepcion2_id}/confirmar"),
        Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    // +$11,06 (delta unitario: 2800 − 1694) × 5 unidades pendientes = +$55,30.
    assert_eq!(saldo(&pool, cliente_id).await, 8470 + 5530);
}

#[sqlx::test(migrations = "./migrations")]
async fn sincronizacion_caja_expone_el_catalogo_completo(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    cargar_items_estandar(&esc).await;
    let (st, _) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{}/confirmar", esc.recepcion_id),
        Some(&esc.token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // El volcado para la PWA trae los productos con precio, IVA y códigos.
    let (st, sync) = pedir(&esc.app, "GET", "/catalogo/sincronizacion-caja", Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK, "{sync}");
    let productos = sync["productos"].as_array().unwrap();
    assert_eq!(productos.len(), 2);

    let yerba = productos.iter().find(|p| p["nombre"] == "Yerba 1kg").unwrap();
    assert_eq!(yerba["precio_actual_centavos"], 1694);
    assert_eq!(yerba["codigos_barras"][0], "7790000000001");
}

#[sqlx::test(migrations = "./migrations")]
async fn confirmar_es_idempotente(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    cargar_items_estandar(&esc).await;

    let uri = format!("/compras/recepciones/{}/confirmar", esc.recepcion_id);
    let (st, _) = pedir(&esc.app, "POST", &uri, Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK);

    // Reintento (la PWA offline puede reintentar): no-op exitoso.
    let (st, resp) = pedir(&esc.app, "POST", &uri, Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK, "reintento debe ser 200: {resp}");
    assert_eq!(resp["ya_estaba_confirmada"], true);

    // Nada duplicado: ni movimientos, ni historial, ni stock.
    let movimientos = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM inventario.movimientos_stock",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(movimientos, 2, "un movimiento por ítem, sin duplicar");

    let stock = sqlx::query_scalar::<_, Decimal>(
        "SELECT cantidad FROM inventario.stock_actual WHERE producto_id = $1",
    )
    .bind(esc.producto_comun)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stock, Decimal::from(10));
}

#[sqlx::test(migrations = "./migrations")]
async fn no_confirma_sin_vencimiento_obligatorio(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Cargar el producto que controla vencimiento SIN fecha debe fallar ya
    // en la carga del ítem.
    let (st, resp) = pedir(
        &esc.app,
        "PUT",
        &format!("/compras/recepciones/{}/items", esc.recepcion_id),
        Some(&esc.token),
        Some(json!({
            "producto_id": esc.producto_con_vencimiento,
            "cantidad": "5",
            "costo_centavos": 800,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
}

#[sqlx::test(migrations = "./migrations")]
async fn no_confirma_recepcion_vacia(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let (st, resp) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{}/confirmar", esc.recepcion_id),
        Some(&esc.token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");

    let estado: String =
        sqlx::query_scalar("SELECT estado::text FROM compras.recepciones WHERE id = $1")
            .bind(esc.recepcion_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(estado, "borrador", "nada aplicado a medias");
}
