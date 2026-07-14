//! Tests del flujo de etiquetado por dispositivo (etiquetadora ESP32):
//! verificación HMAC del middleware y lógica de escaneo/completado.
//! El dispositivo es tonto — solo firma requests; toda la inteligencia
//! (recepción activa, estados, idempotencia) se prueba acá.

mod comun;

use axum::http::StatusCode;
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use comun::{
    app, crear_dispositivo, crear_usuario_con_rol, firma_hmac, pedir, pedir_dispositivo,
    token_para, ROL_ADMINISTRADOR_ID,
};

const DEVICE_ID: &str = "etiquetadora-01";
const SECRETO: &str = "secreto-compartido-de-test";
const RUTA_ESCANEAR: &str = "/etiquetado/escanear";
const RUTA_ESTADO: &str = "/etiquetado/estado";

struct Escenario {
    app: axum::Router,
    producto_id: Uuid,
    codigo_barras: String,
    recepcion_id: Uuid,
}

/// Arma vía API una categoría, DOS productos con código de barras (para que
/// la recepción no se complete al etiquetar solo el primero, y así se pueda
/// probar la idempotencia con la recepción todavía activa), una recepción
/// con ambos ítems y la confirma. Además siembra el dispositivo autenticado.
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
    assert_eq!(st, StatusCode::OK, "{cat}");
    let categoria_id = cat["id"].as_str().unwrap().to_string();

    let codigo_barras = "7790000000001".to_string();
    let (st, prod) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Yerba 1kg",
            "categoria_id": categoria_id,
            "codigos_barras": [codigo_barras],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod}");
    let producto_id: Uuid = prod["id"].as_str().unwrap().parse().unwrap();

    let (st, prod_relleno) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Azúcar 1kg",
            "categoria_id": categoria_id,
            "codigos_barras": ["7790000000002"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod_relleno}");
    let producto_relleno_id: Uuid = prod_relleno["id"].as_str().unwrap().parse().unwrap();

    let (st, rec) = pedir(&app, "POST", "/compras/recepciones", Some(&token), Some(json!({}))).await;
    assert_eq!(st, StatusCode::OK, "{rec}");
    let recepcion_id: Uuid = rec["id"].as_str().unwrap().parse().unwrap();

    for (pid, costo) in [(producto_id, 1000), (producto_relleno_id, 800)] {
        let (st, item) = pedir(
            &app,
            "PUT",
            &format!("/compras/recepciones/{recepcion_id}/items"),
            Some(&token),
            Some(json!({ "producto_id": pid, "cantidad": "10", "costo_centavos": costo })),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{item}");
    }

    let (st, resp) = pedir(
        &app,
        "POST",
        &format!("/compras/recepciones/{recepcion_id}/confirmar"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    crear_dispositivo(pool, DEVICE_ID, SECRETO).await;

    Escenario { app, producto_id, codigo_barras, recepcion_id }
}

fn ahora() -> i64 {
    Utc::now().timestamp()
}

// ---------- Middleware HMAC ----------

#[sqlx::test(migrations = "./migrations")]
async fn firma_valida_permite_el_acceso(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let ts = ahora();
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts, "");

    let (st, resp) = pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, DEVICE_ID, ts, &firma, "").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
}

#[sqlx::test(migrations = "./migrations")]
async fn firma_invalida_es_401(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let ts = ahora();
    // Firma con un secreto distinto al del dispositivo: no coincide.
    let firma = firma_hmac("secreto-equivocado", "GET", RUTA_ESTADO, ts, "");

    let (st, _) = pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, DEVICE_ID, ts, &firma, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn timestamp_vencido_es_401(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    // 301 segundos en el pasado: fuera de la ventana anti-replay, aunque la
    // firma sea correcta para ese timestamp.
    let ts_pasado = ahora() - 301;
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts_pasado, "");
    let (st, _) =
        pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, DEVICE_ID, ts_pasado, &firma, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "301s en el pasado debe rechazarse");

    // 301 segundos en el futuro: misma ventana, otra dirección.
    let ts_futuro = ahora() + 301;
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts_futuro, "");
    let (st, _) =
        pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, DEVICE_ID, ts_futuro, &firma, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "301s en el futuro debe rechazarse");
}

#[sqlx::test(migrations = "./migrations")]
async fn dispositivo_inexistente_es_401(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    let ts = ahora();
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts, "");

    let (st, _) =
        pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, "etiquetadora-fantasma", ts, &firma, "")
            .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn dispositivo_inactivo_es_401(pool: PgPool) {
    let esc = armar_escenario(&pool).await;
    sqlx::query("UPDATE identidad.dispositivos SET activo = false WHERE device_id = $1")
        .bind(DEVICE_ID)
        .execute(&pool)
        .await
        .unwrap();

    let ts = ahora();
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts, "");
    let (st, _) = pedir_dispositivo(&esc.app, "GET", RUTA_ESTADO, DEVICE_ID, ts, &firma, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "un dispositivo desactivado pierde acceso");
}

// ---------- Flujo de escaneo ----------

async fn escanear(app: &axum::Router, codigo: &str) -> (StatusCode, serde_json::Value) {
    let body = json!({ "codigo": codigo }).to_string();
    let ts = ahora();
    let firma = firma_hmac(SECRETO, "POST", RUTA_ESCANEAR, ts, &body);
    pedir_dispositivo(app, "POST", RUTA_ESCANEAR, DEVICE_ID, ts, &firma, &body).await
}

#[sqlx::test(migrations = "./migrations")]
async fn sin_recepcion_activa_cuando_no_hay_nada_pendiente(pool: PgPool) {
    // Dispositivo sembrado pero sin ninguna recepción confirmada en la base.
    let admin = crear_usuario_con_rol(&pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let _token = token_para(admin);
    let app = app(pool.clone());
    crear_dispositivo(&pool, DEVICE_ID, SECRETO).await;

    let (st, resp) = escanear(&app, "7790000000001").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "sin_recepcion_activa");
}

#[sqlx::test(migrations = "./migrations")]
async fn codigo_desconocido_cuando_el_barcode_no_existe(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let (st, resp) = escanear(&esc.app, "0000000000000").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "codigo_desconocido");
}

#[sqlx::test(migrations = "./migrations")]
async fn no_en_recepcion_cuando_el_producto_no_pertenece_a_la_recepcion_activa(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let admin = crear_usuario_con_rol(&pool, "admin-otro", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);

    let (st, cat) = pedir(
        &esc.app,
        "POST",
        "/catalogo/categorias",
        Some(&token),
        Some(json!({ "nombre": "Otra", "markup_pct": "40.00", "iva_pct": "21.00" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cat}");

    // Producto con código de barras propio, pero que nunca se cargó en
    // ninguna recepción.
    let (st, prod) = pedir(
        &esc.app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Fideos 500g",
            "categoria_id": cat["id"],
            "codigos_barras": ["7790000000099"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod}");

    let (st, resp) = escanear(&esc.app, "7790000000099").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "no_en_recepcion");

    let _ = esc.producto_id;
    let _ = esc.recepcion_id;
}

#[sqlx::test(migrations = "./migrations")]
async fn ok_y_ya_etiquetado_son_idempotentes_sin_efectos_duplicados(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let (st, resp) = escanear(&esc.app, &esc.codigo_barras).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "ok");
    assert_eq!(resp["nombre"], "Yerba 1kg");
    assert_eq!(resp["codigo_barras"], esc.codigo_barras);
    // Queda el segundo ítem de la recepción sin etiquetar.
    assert_eq!(resp["pendientes_restantes"], 1);
    // Sin proveedor, costo_incluye_iva default es true: base = costo
    // (1000) × markup 1.40 = 1400.
    assert_eq!(resp["precio"], "$14");

    let (dispositivo_id_1, etiquetado_en_1): (Option<Uuid>, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT etiquetado_por_dispositivo_id, etiquetado_en FROM compras.recepcion_items
             WHERE recepcion_id = $1 AND producto_id = $2",
        )
        .bind(esc.recepcion_id)
        .bind(esc.producto_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(dispositivo_id_1.is_some(), "queda atribuido al dispositivo");

    // La recepción sigue confirmada: todavía queda el segundo ítem.
    let estado_recepcion: String =
        sqlx::query_scalar("SELECT estado::text FROM compras.recepciones WHERE id = $1")
            .bind(esc.recepcion_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(estado_recepcion, "confirmada");

    // Reintento (el firmware puede reenviar el mismo escaneo): sin
    // duplicar nada, ni re-marcar el timestamp/dispositivo.
    let (st, resp) = escanear(&esc.app, &esc.codigo_barras).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "ya_etiquetado");

    let (dispositivo_id_2, etiquetado_en_2): (Option<Uuid>, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT etiquetado_por_dispositivo_id, etiquetado_en FROM compras.recepcion_items
             WHERE recepcion_id = $1 AND producto_id = $2",
        )
        .bind(esc.recepcion_id)
        .bind(esc.producto_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dispositivo_id_1, dispositivo_id_2, "no se reasigna el dispositivo");
    assert_eq!(etiquetado_en_1, etiquetado_en_2, "no se reescribe el timestamp");
}

#[sqlx::test(migrations = "./migrations")]
async fn el_ultimo_pendiente_completa_la_recepcion(pool: PgPool) {
    let esc = armar_escenario(&pool).await;

    let admin = crear_usuario_con_rol(&pool, "admin-otro", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);

    // Segundo producto cargado en la MISMA recepción, todavía en borrador
    // al momento de crearlo... pero la recepción ya está confirmada, así
    // que armamos una segunda recepción con dos ítems para probar el
    // "último pendiente completa la recepción" de punta a punta.
    let (st, cat) = pedir(
        &esc.app,
        "POST",
        "/catalogo/categorias",
        Some(&token),
        Some(json!({ "nombre": "Bebidas", "markup_pct": "30.00", "iva_pct": "21.00" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cat}");
    let categoria_id = cat["id"].as_str().unwrap().to_string();

    let (st, prod_a) = pedir(
        &esc.app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Agua 500ml",
            "categoria_id": categoria_id,
            "codigos_barras": ["7790000000010"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod_a}");
    let producto_a: Uuid = prod_a["id"].as_str().unwrap().parse().unwrap();

    let (st, prod_b) = pedir(
        &esc.app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Gaseosa 1.5L",
            "categoria_id": categoria_id,
            "codigos_barras": ["7790000000020"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod_b}");
    let producto_b: Uuid = prod_b["id"].as_str().unwrap().parse().unwrap();

    let (st, rec) = pedir(&esc.app, "POST", "/compras/recepciones", Some(&token), Some(json!({}))).await;
    assert_eq!(st, StatusCode::OK, "{rec}");
    let recepcion_id: Uuid = rec["id"].as_str().unwrap().parse().unwrap();

    for producto_id in [producto_a, producto_b] {
        let (st, item) = pedir(
            &esc.app,
            "PUT",
            &format!("/compras/recepciones/{recepcion_id}/items"),
            Some(&token),
            Some(json!({
                "producto_id": producto_id,
                "cantidad": "5",
                "costo_centavos": 500,
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{item}");
    }

    let (st, resp) = pedir(
        &esc.app,
        "POST",
        &format!("/compras/recepciones/{recepcion_id}/confirmar"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");

    // Esta recepción es más nueva que la del escenario base, así que es la
    // que queda "activa" para el dispositivo.
    let (st, resp) = escanear(&esc.app, "7790000000010").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "ok");
    assert_eq!(resp["pendientes_restantes"], 1);

    let estado_recepcion: String =
        sqlx::query_scalar("SELECT estado::text FROM compras.recepciones WHERE id = $1")
            .bind(recepcion_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(estado_recepcion, "confirmada", "todavía queda un pendiente");

    let (st, resp) = escanear(&esc.app, "7790000000020").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["estado"], "ok");
    assert_eq!(resp["pendientes_restantes"], 0);

    let estado_recepcion: String =
        sqlx::query_scalar("SELECT estado::text FROM compras.recepciones WHERE id = $1")
            .bind(recepcion_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(estado_recepcion, "completada", "el último pendiente la completa");
}

// ---------- GET /etiquetado/estado ----------

#[sqlx::test(migrations = "./migrations")]
async fn estado_reporta_pendientes_de_la_recepcion_activa_y_menos_uno_si_no_hay(pool: PgPool) {
    let admin = crear_usuario_con_rol(&pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());
    crear_dispositivo(&pool, DEVICE_ID, SECRETO).await;

    let ts = ahora();
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts, "");
    let (st, resp) = pedir_dispositivo(&app, "GET", RUTA_ESTADO, DEVICE_ID, ts, &firma, "").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["pendientes"], -1, "sin recepción activa");

    let (st, cat) = pedir(
        &app,
        "POST",
        "/catalogo/categorias",
        Some(&token),
        Some(json!({ "nombre": "Almacén", "markup_pct": "40.00", "iva_pct": "21.00" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{cat}");
    let (st, prod) = pedir(
        &app,
        "POST",
        "/catalogo/productos",
        Some(&token),
        Some(json!({
            "nombre": "Yerba 1kg",
            "categoria_id": cat["id"],
            "codigos_barras": ["7790000000001"],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{prod}");

    let (st, rec) = pedir(&app, "POST", "/compras/recepciones", Some(&token), Some(json!({}))).await;
    assert_eq!(st, StatusCode::OK, "{rec}");
    let recepcion_id = rec["id"].as_str().unwrap();

    let (st, item) = pedir(
        &app,
        "PUT",
        &format!("/compras/recepciones/{recepcion_id}/items"),
        Some(&token),
        Some(json!({ "producto_id": prod["id"], "cantidad": "10", "costo_centavos": 1000 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{item}");

    let (st, _) = pedir(
        &app,
        "POST",
        &format!("/compras/recepciones/{recepcion_id}/confirmar"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let ts = ahora();
    let firma = firma_hmac(SECRETO, "GET", RUTA_ESTADO, ts, "");
    let (st, resp) = pedir_dispositivo(&app, "GET", RUTA_ESTADO, DEVICE_ID, ts, &firma, "").await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(resp["pendientes"], 1);
}
