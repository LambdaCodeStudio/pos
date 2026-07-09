//! Tests de Fase 2: ajustes de inventario, conteos, validación de
//! disponibilidad, idempotencia y alertas de vencimiento.

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
    producto_id: Uuid,
}

/// Categoría + producto simple, listo para ajustar.
async fn armar_escenario(pool: &PgPool) -> Escenario {
    let admin = crear_usuario_con_rol(pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (st, cat) = pedir(
        &app,
        "POST",
        "/catalogo/categorias",
        Some(&token),
        Some(json!({ "nombre": "Almacén" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cat}");

    let (st, prod) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({ "nombre": "Arroz 1kg", "categoria_id": cat["id"] })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod}");

    Escenario {
        app,
        token,
        producto_id: prod["id"].as_str().unwrap().parse().unwrap(),
    }
}

async fn stock_de(pool: &PgPool, producto_id: Uuid) -> Decimal {
    sqlx::query_scalar::<_, Decimal>(
        "SELECT COALESCE((SELECT cantidad FROM inventario.stock_actual WHERE producto_id = $1), 0)",
    )
    .bind(producto_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn conteo_calcula_delta_contra_proyeccion(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Conteo inicial: proyección 0, contado 50 → delta +50.
    let (st, resp) = pedir(
        &esc.app,
        "POST",
        "/inventario/ajustes",
        Some(&esc.token),
        Some(json!({
            "motivo": "conteo",
            "items": [{ "producto_id": esc.producto_id, "cantidad_contada": "50" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(50));

    // Segundo conteo: faltan 3 (robo descubierto en el conteo) → delta -3.
    let (st, resp) = pedir(
        &esc.app,
        "POST",
        "/inventario/ajustes",
        Some(&esc.token),
        Some(json!({
            "motivo": "robo",
            "items": [{ "producto_id": esc.producto_id, "cantidad_contada": "47" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let delta: Decimal = resp["movimientos"][0]["delta"].as_str().unwrap().parse().unwrap();
    assert_eq!(delta, Decimal::from(-3));
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(47));

    // El ledger reconstruye la proyección: SUM = 50 - 3 = 47.
    let suma = sqlx::query_scalar::<_, Decimal>(
        "SELECT SUM(cantidad) FROM inventario.movimientos_stock WHERE producto_id = $1",
    )
    .bind(esc.producto_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(suma, Decimal::from(47));
}

#[sqlx::test(migrations = "./migrations")]
async fn ajuste_negativo_valida_disponibilidad(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Stock 5 por conteo.
    let (st, _) = pedir(
        &esc.app,
        "POST",
        "/inventario/ajustes",
        Some(&esc.token),
        Some(json!({
            "motivo": "conteo",
            "items": [{ "producto_id": esc.producto_id, "cantidad_contada": "5" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Rotura de 10 con stock 5: los ajustes SÍ validan disponibilidad
    // (asimetría deliberada con las ventas).
    let (st, resp) = pedir(
        &esc.app,
        "POST",
        "/inventario/ajustes",
        Some(&esc.token),
        Some(json!({
            "motivo": "rotura",
            "items": [{ "producto_id": esc.producto_id, "delta": "-10" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(5), "nada aplicado");
}

#[sqlx::test(migrations = "./migrations")]
async fn ajuste_es_idempotente_por_uuid(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let ajuste_id = Uuid::now_v7();

    let cuerpo = json!({
        "id": ajuste_id,
        "motivo": "conteo",
        "items": [{ "producto_id": esc.producto_id, "cantidad_contada": "20" }],
    });

    let (st, _) = pedir(&esc.app, "POST", "/inventario/ajustes", Some(&esc.token), Some(cuerpo.clone())).await;
    assert_eq!(st, StatusCode::OK);

    // Reintento del cliente offline con el mismo UUID: no reaplica.
    let (st, resp) = pedir(&esc.app, "POST", "/inventario/ajustes", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_aplicado"], true);
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(20));
}

#[sqlx::test(migrations = "./migrations")]
async fn ajuste_de_lote_y_alertas_de_vencimiento(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Producto con vencimiento, recibido por recepción confirmada → crea lote.
    let (st, prod) = pedir(
        &esc.app,
        "POST",
        "/catalogo/productos",
        Some(&esc.token),
        Some(json!({
            "nombre": "Yogur bebible",
            "categoria_id": sqlx::query_scalar::<_, Uuid>("SELECT id FROM catalogo.categorias LIMIT 1")
                .fetch_one(&pool).await.unwrap(),
            "controla_vencimiento": true,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod}");
    let producto_vto: Uuid = prod["id"].as_str().unwrap().parse().unwrap();

    let (_, rec) = pedir(&esc.app, "POST", "/compras/recepciones", Some(&esc.token), Some(json!({}))).await;
    let recepcion_id = rec["id"].as_str().unwrap().to_string();

    // Vence en 10 días: debe aparecer en la ventana de 30 y no en la de 5.
    let vencimiento = (chrono::Utc::now().date_naive() + chrono::Duration::days(10)).to_string();
    let (st, item) = pedir(
        &esc.app,
        "PUT",
        &format!("/compras/recepciones/{recepcion_id}/items"),
        Some(&esc.token),
        Some(json!({
            "producto_id": producto_vto,
            "cantidad": "12",
            "costo_centavos": 500,
            "vencimiento": vencimiento,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{item}");

    let (st, _) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{recepcion_id}/confirmar"),
        Some(&esc.token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Alerta presente a 30 días…
    let (st, alertas) = pedir(&esc.app, "GET", "/inventario/alertas-vencimiento?dias=30", Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK);
    let alertas = alertas.as_array().unwrap();
    assert_eq!(alertas.len(), 1, "{alertas:?}");
    assert_eq!(alertas[0]["producto_id"].as_str().unwrap(), producto_vto.to_string());
    assert_eq!(alertas[0]["dias_restantes"], 10);
    let lote_id: Uuid = alertas[0]["lote_id"].as_str().unwrap().parse().unwrap();

    // …ausente a 5 días.
    let (_, alertas5) = pedir(&esc.app, "GET", "/inventario/alertas-vencimiento?dias=5", Some(&esc.token), None).await;
    assert_eq!(alertas5.as_array().unwrap().len(), 0);

    // Se tiran 12 unidades vencidas del lote → lote y stock en 0.
    let (st, resp) = pedir(
        &esc.app,
        "POST",
        "/inventario/ajustes",
        Some(&esc.token),
        Some(json!({
            "motivo": "vencimiento",
            "observaciones": "se tira el lote",
            "items": [{ "producto_id": producto_vto, "lote_id": lote_id, "delta": "-12" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    let lote_restante = sqlx::query_scalar::<_, Decimal>(
        "SELECT cantidad_actual FROM inventario.lotes WHERE id = $1",
    )
    .bind(lote_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(lote_restante, Decimal::ZERO);
    assert_eq!(stock_de(&pool, producto_vto).await, Decimal::ZERO);

    // Sin stock, la alerta desaparece.
    let (_, alertas) = pedir(&esc.app, "GET", "/inventario/alertas-vencimiento?dias=30", Some(&esc.token), None).await;
    assert_eq!(alertas.as_array().unwrap().len(), 0);

    // Y el movimiento quedó ligado al lote y al ajuste en el ledger.
    let (tipo, ajuste_ref): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT tipo::text, ajuste_id FROM inventario.movimientos_stock
         WHERE lote_id = $1 AND cantidad < 0",
    )
    .bind(lote_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tipo, "ajuste");
    assert!(ajuste_ref.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn ajustar_requiere_permiso(pool: PgPool) {
    let cajero = crear_usuario_con_rol(&pool, "cajero", comun::ROL_CAJERO_ID).await;
    let token = token_para(cajero);
    let app = app(pool);

    let (st, _) = pedir(
        &app,
        "POST",
        "/inventario/ajustes",
        Some(&token),
        Some(json!({
            "motivo": "otro",
            "items": [{ "producto_id": Uuid::now_v7(), "delta": "1" }],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
