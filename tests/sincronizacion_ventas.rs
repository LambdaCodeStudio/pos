//! Tests de Fase 3: sincronización idempotente de ventas offline, asignación
//! FEFO, stock negativo permitido, invariante de pagos, anulación y arqueo.

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
    sesion_id: Uuid,
}

async fn armar_escenario(pool: &PgPool) -> Escenario {
    let admin = crear_usuario_con_rol(pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (st, cat) = pedir(&app, "POST", "/catalogo/categorias", Some(&token),
        Some(json!({ "nombre": "Almacén" }))).await;
    assert_eq!(st, StatusCode::OK, "{cat}");

    let (st, prod) = pedir(&app, "POST", "/catalogo/productos", Some(&token),
        Some(json!({ "nombre": "Gaseosa 2L", "categoria_id": cat["id"] }))).await;
    assert_eq!(st, StatusCode::OK, "{prod}");

    let (st, sesion) = pedir(&app, "POST", "/ventas/sesiones", Some(&token),
        Some(json!({ "monto_inicial_centavos": 10000 }))).await;
    assert_eq!(st, StatusCode::OK, "{sesion}");

    Escenario {
        app,
        token,
        producto_id: prod["id"].as_str().unwrap().parse().unwrap(),
        sesion_id: sesion["id"].as_str().unwrap().parse().unwrap(),
    }
}

fn venta_simple(esc: &Escenario, venta_id: Uuid, cantidad: &str, total: i64) -> serde_json::Value {
    json!({
        "id": venta_id,
        "sesion_id": esc.sesion_id,
        "total_centavos": total,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_id,
            "precio_unitario_centavos": 1000,
            "cantidad": cantidad,
            "subtotal_centavos": total,
        }],
        "pagos": [{ "medio": "efectivo", "monto_centavos": total }],
    })
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
async fn sincronizacion_es_idempotente(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let venta_id = Uuid::now_v7();
    let cuerpo = venta_simple(&esc, venta_id, "3", 3000);

    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo.clone())).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_sincronizada"], false);

    // Reintento del dispositivo (se cortó la conexión antes del ACK).
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(cuerpo)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_sincronizada"], true);

    // Nada duplicado: una venta, un ítem, un pago, un movimiento, stock -3.
    let ventas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ventas.ventas")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(ventas, 1);
    let movimientos: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventario.movimientos_stock WHERE tipo = 'salida_venta'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(movimientos, 1);
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(-3));
}

#[sqlx::test(migrations = "./migrations")]
async fn fefo_descuenta_del_lote_mas_proximo_en_cascada(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Producto con vencimiento y DOS lotes: el que vence antes (4 u.) llegó
    // después; FEFO igual lo consume primero.
    let categoria: Uuid = sqlx::query_scalar("SELECT id FROM catalogo.categorias LIMIT 1")
        .fetch_one(&pool).await.unwrap();
    let (st, prod) = pedir(&esc.app, "POST", "/catalogo/productos", Some(&esc.token),
        Some(json!({ "nombre": "Queso cremoso", "categoria_id": categoria, "controla_vencimiento": true, "unidad_de_venta": "peso" }))).await;
    assert_eq!(st, StatusCode::OK, "{prod}");
    let producto: Uuid = prod["id"].as_str().unwrap().parse().unwrap();

    let hoy = chrono::Utc::now().date_naive();
    for (dias, cantidad) in [(30i64, "10"), (5, "4")] {
        let (_, rec) = pedir(&esc.app, "POST", "/compras/recepciones", Some(&esc.token), Some(json!({}))).await;
        let rec_id = rec["id"].as_str().unwrap().to_string();
        let (st, item) = pedir(&esc.app, "PUT", &format!("/compras/recepciones/{rec_id}/items"),
            Some(&esc.token), Some(json!({
                "producto_id": producto,
                "cantidad": cantidad,
                "costo_centavos": 2000,
                "vencimiento": (hoy + chrono::Duration::days(dias)).to_string(),
            }))).await;
        assert_eq!(st, StatusCode::OK, "{item}");
        let (st, _) = pedir(&esc.app, "POST", &format!("/compras/recepciones/{rec_id}/confirmar"),
            Some(&esc.token), None).await;
        assert_eq!(st, StatusCode::OK);
    }
    assert_eq!(stock_de(&pool, producto).await, Decimal::from(14));

    // Venta de 6: FEFO toma 4 del lote que vence en 5 días y 2 del de 30.
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(json!({
        "id": Uuid::now_v7(),
        "sesion_id": esc.sesion_id,
        "total_centavos": 6000,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": producto,
            "precio_unitario_centavos": 1000,
            "cantidad": "6",
            "subtotal_centavos": 6000,
        }],
        "pagos": [{ "medio": "tarjeta", "monto_centavos": 6000 }],
    }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    let lotes: Vec<(Decimal,)> = sqlx::query_as(
        "SELECT cantidad_actual FROM inventario.lotes WHERE producto_id = $1 ORDER BY vencimiento",
    ).bind(producto).fetch_all(&pool).await.unwrap();
    assert_eq!(lotes[0].0, Decimal::ZERO, "el lote más próximo se agota primero");
    assert_eq!(lotes[1].0, Decimal::from(8), "el resto sale del siguiente");
    assert_eq!(stock_de(&pool, producto).await, Decimal::from(8));

    // Dos movimientos de salida (uno por lote), suma -6.
    let suma: Decimal = sqlx::query_scalar(
        "SELECT SUM(cantidad) FROM inventario.movimientos_stock
         WHERE producto_id = $1 AND tipo = 'salida_venta'",
    ).bind(producto).fetch_one(&pool).await.unwrap();
    assert_eq!(suma, Decimal::from(-6));
}

#[sqlx::test(migrations = "./migrations")]
async fn stock_negativo_permitido_en_ventas(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Sin stock: la caja jamás bloquea con el cliente en el mostrador.
    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token),
        Some(venta_simple(&esc, Uuid::now_v7(), "5", 5000))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(-5));
}

#[sqlx::test(migrations = "./migrations")]
async fn suma_de_pagos_debe_igualar_total(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let (st, resp) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(json!({
        "id": Uuid::now_v7(),
        "sesion_id": esc.sesion_id,
        "total_centavos": 3000,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_id,
            "precio_unitario_centavos": 1000,
            "cantidad": "3",
            "subtotal_centavos": 3000,
        }],
        "pagos": [{ "medio": "efectivo", "monto_centavos": 2000 }],
    }))).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");

    // Nada persistido: atómica, todo o nada.
    let ventas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ventas.ventas")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(ventas, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn anulacion_revierte_stock_y_es_idempotente(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let venta_id = Uuid::now_v7();

    let (st, _) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token),
        Some(venta_simple(&esc, venta_id, "3", 3000))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::from(-3));

    let (st, resp) = pedir(&esc.app, "POST", &format!("/ventas/{venta_id}/anular"),
        Some(&esc.token), Some(json!({ "motivo": "cliente se arrepintió" }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_anulada"], false);
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::ZERO, "stock restituido");

    // El ledger conserva original + contra-asiento (jamás se borra nada).
    let movimientos: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventario.movimientos_stock WHERE tipo = 'salida_venta'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(movimientos, 2);

    // Reintento: no-op, sin duplicar la reversa.
    let (st, resp) = pedir(&esc.app, "POST", &format!("/ventas/{venta_id}/anular"),
        Some(&esc.token), None).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_anulada"], true);
    assert_eq!(stock_de(&pool, esc.producto_id).await, Decimal::ZERO);

    let estado: String = sqlx::query_scalar("SELECT estado::text FROM ventas.ventas WHERE id = $1")
        .bind(venta_id).fetch_one(&pool).await.unwrap();
    assert_eq!(estado, "anulada");
}

#[sqlx::test(migrations = "./migrations")]
async fn arqueo_registra_diferencia_sin_corregirla(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // Venta mixta: 3000 efectivo + 1000 tarjeta. Solo el efectivo cuenta
    // para el arqueo.
    let (st, _) = pedir(&esc.app, "POST", "/ventas", Some(&esc.token), Some(json!({
        "id": Uuid::now_v7(),
        "sesion_id": esc.sesion_id,
        "total_centavos": 4000,
        "vendida_en": chrono::Utc::now(),
        "items": [{
            "producto_id": esc.producto_id,
            "precio_unitario_centavos": 1000,
            "cantidad": "4",
            "subtotal_centavos": 4000,
        }],
        "pagos": [
            { "medio": "efectivo", "monto_centavos": 3000 },
            { "medio": "tarjeta", "monto_centavos": 1000 },
        ],
    }))).await;
    assert_eq!(st, StatusCode::OK);

    // Esperado: 10000 inicial + 3000 efectivo = 13000. Contado: 12800.
    let (st, resp) = pedir(&esc.app, "POST", &format!("/ventas/sesiones/{}/cerrar", esc.sesion_id),
        Some(&esc.token), Some(json!({ "monto_contado_centavos": 12800 }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["efectivo_esperado_centavos"], 13000);
    assert_eq!(resp["diferencia_arqueo_centavos"], -200, "faltante registrado tal cual");

    // Cierre idempotente: devuelve el arqueo ya registrado.
    let (st, resp) = pedir(&esc.app, "POST", &format!("/ventas/sesiones/{}/cerrar", esc.sesion_id),
        Some(&esc.token), Some(json!({ "monto_contado_centavos": 99999 }))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["ya_estaba_cerrada"], true);
    assert_eq!(resp["diferencia_arqueo_centavos"], -200, "el arqueo original no se pisa");
}
