//! Utilidades compartidas por los tests de integración. Cada test corre en
//! una base efímera creada por #[sqlx::test] con las migraciones aplicadas.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub const JWT_SECRET_TEST: &str = "secreto-de-test";

// Cada binario de tests compila este módulo por separado; no todos usan
// ambas constantes.
#[allow(dead_code)]
pub const ROL_ADMINISTRADOR_ID: Uuid = Uuid::from_u128(0x01900000_0000_7000_8000_000000000001);
#[allow(dead_code)]
pub const ROL_CAJERO_ID: Uuid = Uuid::from_u128(0x01900000_0000_7000_8000_000000000003);

pub fn app(pool: PgPool) -> Router {
    // Que los tracing::error del backend se vean al correr con --nocapture.
    let _ = tracing_subscriber::fmt().with_env_filter("pos=debug").try_init();
    let estado = pos::estado::Estado::nuevo(pool, JWT_SECRET_TEST.to_string());
    pos::armar_router(estado)
}

/// Crea un usuario directo en la base (el hash no importa: autenticamos por JWT).
pub async fn crear_usuario_con_rol(pool: &PgPool, nombre: &str, rol_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO identidad.usuarios (id, nombre, password_hash, rol_id)
         VALUES ($1, $2, 'hash-irrelevante', $3)",
    )
    .bind(id)
    .bind(nombre)
    .bind(rol_id)
    .execute(pool)
    .await
    .expect("crear usuario de test");
    id
}

pub fn token_para(usuario_id: Uuid) -> String {
    pos::identidad::auth::emitir_token(usuario_id, JWT_SECRET_TEST).expect("emitir token")
}

/// Ejecuta un request JSON contra el router y devuelve (status, cuerpo).
pub async fn pedir(
    app: &Router,
    metodo: &str,
    uri: &str,
    token: Option<&str>,
    cuerpo: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(metodo).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = match cuerpo {
        Some(json) => req
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };

    let resp = app.clone().oneshot(req).await.expect("ejecutar request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// Crea un dispositivo de etiquetado (contexto Identidad) con secreto
/// conocido en texto claro, tal como lo necesita el middleware HMAC.
#[allow(dead_code)]
pub async fn crear_dispositivo(pool: &PgPool, device_id: &str, secreto: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO identidad.dispositivos (id, device_id, secreto_hmac) VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(device_id)
    .bind(secreto)
    .execute(pool)
    .await
    .expect("crear dispositivo de test");
    id
}

/// Recalcula la firma tal como la calcularía el firmware: HMAC-SHA256 sobre
/// "MÉTODO\nRUTA\nTIMESTAMP\nBODY", en hex minúsculas.
#[allow(dead_code)]
pub fn firma_hmac(secreto: &str, metodo: &str, ruta: &str, timestamp: i64, body: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let canonica = format!("{metodo}\n{ruta}\n{timestamp}\n{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secreto.as_bytes()).expect("clave hmac");
    mac.update(canonica.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Ejecuta un request firmado como lo haría la etiquetadora ESP32. La firma
/// se recibe ya calculada (no se recalcula acá) para que los tests puedan
/// tamperearla deliberadamente.
#[allow(dead_code)]
pub async fn pedir_dispositivo(
    app: &Router,
    metodo: &str,
    ruta: &str,
    device_id: &str,
    timestamp: i64,
    firma: &str,
    body: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(metodo)
        .uri(ruta)
        .header("x-device-id", device_id)
        .header("x-timestamp", timestamp.to_string())
        .header("x-signature", firma);
    if !body.is_empty() {
        req = req.header("content-type", "application/json");
    }
    let req = req.body(Body::from(body.to_string())).unwrap();

    let resp = app.clone().oneshot(req).await.expect("ejecutar request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}
