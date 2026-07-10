//! Tests de Fase 4: cuenta corriente (libreta de fiado). Cargo en la misma
//! transacción de la venta, límite que SÍ bloquea, pagos idempotentes,
//! saldo reconstruible del ledger y reversa en anulación.

mod comun;

use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use comun::{app, crear_usuario_con_rol, pedir, token_para, ROL_ADMINISTRADOR_ID, ROL_CAJERO_ID};

struct Escenario {
    app: axum::Router,
    token: String,
    producto_id: Uuid,
    sesion_id: Uuid,
    cliente_id: Uuid,
}

/// Cliente con límite de crédito de $50,00.
async fn armar_escenario(pool: &PgPool) -> Escenario {
    let admin = crear_usuario_con_rol(pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (st, cat) = pedir(&app, "POST", "/catalogo/categorias", Some(&token),
        Some(json!({ "nombre": "Almacén" }))).await;
    assert_eq!(st, StatusCode::OK, "{cat}");
    let (st, prod) = pedir(&app, "POST", "/catalogo/productos", Some(&token),
        Some(json!({ "nombre": "Pan lactal", "categoria_id": cat["id"] }))).await;
    assert_eq!(st, StatusCode::OK, "{prod}");
    let (st, sesion) = pedir(&app, "POST", "/ventas/sesiones", Some(&token),
        Some(json!({ "monto_inicial_centavos": 0 }))).await;
    assert_eq!(st, StatusCode::OK, "{sesion}");
    let (st, cliente) = pedir(&app, "POST", "/clientes", Some(&token),
        Some(json!({ "nombre": "Doña Rosa", "limite_credito_centavos": 5000 }))).await;
    assert_eq!(st, StatusCode::OK, "{cliente}");

    Escenario {
        app,
        token,
        producto_id: prod["id"].as_str().unwrap().parse().unwrap(),
        sesion_id: sesion["id"].as_str().unwrap().parse().unwrap(),
        cliente_id: cliente["id"].as_str().unwrap().parse().unwrap(),
    }
}

fn venta_con_pagos(
    esc: &Escenario,
    sesion_id: Uuid,
    total: i64,
    cliente_id: Option<Uuid>,
    pagos: Value,
) -> Value {
    json!({
        "id": Uuid::now_v7(),
        "sesion_id": sesion_id,
        "cliente_id": cliente_id,
        "total_centavos": total,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_id,
            "precio_unitario_centavos": total,
            "cantidad": "1",
            "subtotal_centavos": total,
        }],
        "pagos": pagos,
    })
}

async fn saldo_de(pool: &PgPool, cliente_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT saldo_actual_centavos FROM clientes.clientes WHERE id = $1",
    )
    .bind(cliente_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn venta_fiada_inserta_cargo_en_la_misma_transaccion(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // El fiado va por el ticket completo (no se mezcla con otro medio).
    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 3000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 3000 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 3000);

    // El cargo referencia la venta (la cuenta NO duplica productos).
    let (tipo, venta_ref): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT tipo::text, venta_id FROM clientes.cuenta_movimientos WHERE cliente_id = $1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tipo, "cargo");
    assert!(venta_ref.is_some());

    // La proyección es reconstruible: SUM(ledger) == saldo.
    let suma: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(monto_centavos), 0)::bigint FROM clientes.cuenta_movimientos WHERE cliente_id = $1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(suma, 3000);

    // El renglón de producto queda pendiente por la cantidad completa.
    let (cantidad, cantidad_pendiente): (Decimal, Decimal) = sqlx::query_as(
        "SELECT cantidad, cantidad_pendiente FROM clientes.cargo_items WHERE cliente_id = $1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cantidad, Decimal::ONE);
    assert_eq!(cantidad_pendiente, Decimal::ONE);
}

#[sqlx::test(migrations = "./migrations")]
async fn fiado_mezclado_con_otro_medio_es_invalido(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // El fiado no se puede combinar con otro medio: se rechaza, aunque la
    // suma de pagos coincida con el total.
    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 4000, Some(esc.cliente_id), json!([
        { "medio": "efectivo", "monto_centavos": 1000 },
        { "medio": "cuenta_corriente", "monto_centavos": 3000 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn limite_de_credito_bloquea_sin_permiso(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Cajero: tiene vender/abrir_caja pero NO exceder_limite_credito.
    let cajero = crear_usuario_con_rol(&pool, "cajero", ROL_CAJERO_ID).await;
    let token_cajero = token_para(cajero);
    let (st, sesion) = pedir(&esc.app, "POST", "/ventas/sesiones", Some(&token_cajero),
        Some(json!({ "monto_inicial_centavos": 0 }))).await;
    assert_eq!(st, StatusCode::OK, "{sesion}");
    let sesion_cajero: Uuid = sesion["id"].as_str().unwrap().parse().unwrap();

    // Fiado de $60,00 con límite de $50,00 → bloquea (403) y no persiste nada.
    let cuerpo = venta_con_pagos(&esc, sesion_cajero, 6000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 6000 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&token_cajero), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{resp}");

    let ventas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ventas.ventas")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(ventas, 0, "transacción atómica: la venta bloqueada no deja rastro");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 0);

    // Con el permiso individual aditivo, la misma venta pasa.
    sqlx::query("INSERT INTO identidad.usuario_permisos (usuario_id, permiso) VALUES ($1, 'exceder_limite_credito')")
        .bind(cajero).execute(&pool).await.unwrap();
    let cuerpo = venta_con_pagos(&esc, sesion_cajero, 6000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 6000 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&token_cajero), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 6000);
}

#[sqlx::test(migrations = "./migrations")]
async fn cliente_sin_limite_nunca_bloquea(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let (st, cliente) = pedir(&esc.app, "POST", "/clientes", Some(&esc.token),
        Some(json!({ "nombre": "Don Tito" }))).await;
    assert_eq!(st, StatusCode::OK);
    let sin_limite: Uuid = cliente["id"].as_str().unwrap().parse().unwrap();

    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 999_999, Some(sin_limite), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 999_999 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, sin_limite).await, 999_999);
}

#[sqlx::test(migrations = "./migrations")]
async fn fiado_sin_cliente_es_invalido(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 1000, None, json!([
        { "medio": "cuenta_corriente", "monto_centavos": 1000 },
    ]));
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
}

#[sqlx::test(migrations = "./migrations")]
async fn pago_reduce_saldo_y_es_idempotente(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Fiado de $30,00.
    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 3000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 3000 },
    ]));
    let (st, _) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK);

    // Paga $20,00 en efectivo (sin imputación: saldo global corrido).
    let pago_id = Uuid::now_v7();
    let cuerpo_pago = json!({ "id": pago_id, "monto_centavos": 2000, "medio": "efectivo" });
    let (st, resp) = pedir(&esc.app, "POST", &format!("/clientes/{}/pagos", esc.cliente_id),
        Some(&esc.token), Some(cuerpo_pago.clone())).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["saldo_resultante_centavos"], 1000);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 1000);

    // Reintento con el mismo UUID: no-op.
    let (st, resp) = pedir(&esc.app, "POST", &format!("/clientes/{}/pagos", esc.cliente_id),
        Some(&esc.token), Some(cuerpo_pago)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_aplicado"], true);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 1000);
}

#[sqlx::test(migrations = "./migrations")]
async fn anulacion_revierte_el_cargo_del_fiado(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let venta = venta_con_pagos(&esc, esc.sesion_id, 3000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 3000 },
    ]));
    let venta_id = venta["id"].as_str().unwrap().to_string();
    let (st, _) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(venta)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 3000);

    let (st, resp) = pedir(&esc.app, "POST", &format!("/ventas/{venta_id}/anular"),
        Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 0, "el fiado vuelve a cero");

    // Ledger intacto: cargo original + contra-asiento de ajuste.
    let movimientos: Vec<(String, i64)> = sqlx::query_as(
        "SELECT tipo::text, monto_centavos FROM clientes.cuenta_movimientos
         WHERE cliente_id = $1 ORDER BY creado_en",
    )
    .bind(esc.cliente_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(movimientos.len(), 2);
    assert_eq!(movimientos[0], ("cargo".to_string(), 3000));
    assert_eq!(movimientos[1], ("ajuste".to_string(), -3000));

    // El renglón de producto también queda saldado: no debe seguir
    // consumiéndose por FIFO ni revalorizándose si el producto cambia de precio.
    let cantidad_pendiente: Decimal = sqlx::query_scalar(
        "SELECT cantidad_pendiente FROM clientes.cargo_items WHERE cliente_id = $1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cantidad_pendiente, Decimal::ZERO);

    // Reintento de anulación: no duplica la reversa.
    let (st, _) = pedir(&esc.app, "POST", &format!("/ventas/{venta_id}/anular"),
        Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn pago_consume_cargo_items_pendientes_por_fifo(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Precio de catálogo: $10,00/u. Dos ventas fiadas de 2 unidades cada una,
    // la más vieja primero.
    let (st, _) = pedir(&esc.app, "POST", &format!("/catalogo/productos/{}/precio", esc.producto_id),
        Some(&esc.token), Some(json!({ "precio_centavos": 1000 }))).await;
    assert_eq!(st, StatusCode::OK);

    for _ in 0..2 {
        let cuerpo = json!({
            "id": Uuid::now_v7(),
            "sesion_id": esc.sesion_id,
            "cliente_id": esc.cliente_id,
            "total_centavos": 2000,
            "vendida_en": chrono::Utc::now(),
            "items": [{
                "producto_id": esc.producto_id,
                "precio_unitario_centavos": 1000,
                "cantidad": "2",
                "subtotal_centavos": 2000,
            }],
            "pagos": [{ "medio": "cuenta_corriente", "monto_centavos": 2000 }],
        });
        let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
        assert_eq!(st, StatusCode::OK, "{resp}");
    }
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 4000);

    // Paga $25,00: salda el renglón más viejo entero ($20,00) y la mitad
    // del segundo (1 de las 2 unidades pendientes, a $10,00/u).
    let (st, resp) = pedir(&esc.app, "POST", &format!("/clientes/{}/pagos", esc.cliente_id),
        Some(&esc.token), Some(json!({ "monto_centavos": 2500, "medio": "efectivo" }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 1500);

    let pendientes: Vec<Decimal> = sqlx::query_scalar(
        "SELECT cantidad_pendiente FROM clientes.cargo_items WHERE cliente_id = $1 ORDER BY creado_en",
    )
    .bind(esc.cliente_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(pendientes, vec![Decimal::ZERO, Decimal::new(15, 1)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn reprecio_automatico_sube_y_baja_la_deuda_pendiente(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let (st, _) = pedir(&esc.app, "POST", &format!("/catalogo/productos/{}/precio", esc.producto_id),
        Some(&esc.token), Some(json!({ "precio_centavos": 1000 }))).await;
    assert_eq!(st, StatusCode::OK);

    let cuerpo = json!({
        "id": Uuid::now_v7(),
        "sesion_id": esc.sesion_id,
        "cliente_id": esc.cliente_id,
        "total_centavos": 3000,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_id,
            "precio_unitario_centavos": 1000,
            "cantidad": "3",
            "subtotal_centavos": 3000,
        }],
        "pagos": [{ "medio": "cuenta_corriente", "monto_centavos": 3000 }],
    });
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 3000);

    // Sube a $12,00/u: +$2,00 × 3 unidades pendientes = +$6,00.
    let (st, _) = pedir(&esc.app, "POST", &format!("/catalogo/productos/{}/precio", esc.producto_id),
        Some(&esc.token), Some(json!({ "precio_centavos": 1200 }))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 3600);

    // Baja a $7,00/u: −$5,00 × 3 unidades pendientes = −$15,00 (simétrico).
    let (st, _) = pedir(&esc.app, "POST", &format!("/catalogo/productos/{}/precio", esc.producto_id),
        Some(&esc.token), Some(json!({ "precio_centavos": 700 }))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 2100);

    let motivo: Option<String> = sqlx::query_scalar(
        "SELECT motivo FROM clientes.cuenta_movimientos
         WHERE cliente_id = $1 AND tipo = 'ajuste' ORDER BY creado_en LIMIT 1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(motivo, Some(format!("reprecio_producto:{}", esc.producto_id)));
}

#[sqlx::test(migrations = "./migrations")]
async fn condonacion_negativa_tambien_consume_fifo(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // El valor pendiente se calcula al precio de catálogo vigente.
    let (st, _) = pedir(&esc.app, "POST", &format!("/catalogo/productos/{}/precio", esc.producto_id),
        Some(&esc.token), Some(json!({ "precio_centavos": 3000 }))).await;
    assert_eq!(st, StatusCode::OK);

    let cuerpo = venta_con_pagos(&esc, esc.sesion_id, 3000, Some(esc.cliente_id), json!([
        { "medio": "cuenta_corriente", "monto_centavos": 3000 },
    ]));
    let (st, _) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK);

    // Ajuste manual: condona la mitad de la deuda (motivo obligatorio).
    let (st, resp) = pedir(&esc.app, "POST", &format!("/clientes/{}/ajustes", esc.cliente_id),
        Some(&esc.token), Some(json!({
            "monto_centavos": -1500,
            "motivo": "condonacion parcial",
        }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(saldo_de(&pool, esc.cliente_id).await, 1500);

    // El renglón (cantidad = 1 unidad a $30,00) queda pendiente por la mitad.
    let cantidad_pendiente: Decimal = sqlx::query_scalar(
        "SELECT cantidad_pendiente FROM clientes.cargo_items WHERE cliente_id = $1",
    )
    .bind(esc.cliente_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cantidad_pendiente, Decimal::new(5, 1));
}
