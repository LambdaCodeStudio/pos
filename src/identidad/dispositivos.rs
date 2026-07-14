//! Dispositivos de campo (ESP32 etiquetadora) autenticados por HMAC
//! compartido, sin usuario/rol. Contrato de firma acordado con el firmware
//! (ver `esp32.txt`): headers X-Device-Id / X-Timestamp / X-Signature,
//! cadena canónica "MÉTODO\nRUTA\nTIMESTAMP\nBODY", ventana anti-replay de
//! 300 segundos, comparación en tiempo constante.

use axum::extract::{FromRequestParts, OriginalUri, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::permisos;

/// Ventana anti-replay: se rechaza cualquier request cuyo X-Timestamp esté
/// más lejos que esto de la hora del servidor, en cualquier dirección.
const VENTANA_TIMESTAMP_SEGUNDOS: i64 = 300;
/// Los requests firmados son JSON chicos (un código de barras); un límite
/// generoso alcanza y evita que un body enorme agote memoria.
const LIMITE_BODY_BYTES: usize = 64 * 1024;

struct DispositivoFila {
    id: Uuid,
    device_id: String,
    secreto_hmac: String,
}

async fn buscar_activo(pool: &sqlx::PgPool, device_id: &str) -> Result<Option<DispositivoFila>, ErrorApi> {
    let fila = sqlx::query_as!(
        DispositivoFila,
        r#"
        SELECT id, device_id, secreto_hmac
        FROM identidad.dispositivos
        WHERE device_id = $1 AND activo
        "#,
        device_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(fila)
}

/// Compara la firma recibida (hex) contra la recalculada, en tiempo
/// constante (Mac::verify_slice usa `subtle` internamente). Nunca compara
/// strings con `==`.
fn firma_valida(secreto: &str, mensaje: &str, firma_hex_recibida: &str) -> bool {
    let Ok(firma_recibida) = hex::decode(firma_hex_recibida) else {
        return false;
    };
    let mut mac = match Hmac::<Sha256>::new_from_slice(secreto.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(mensaje.as_bytes());
    mac.verify_slice(&firma_recibida).is_ok()
}

/// Dispositivo autenticado del request, inyectado por `verificar_hmac`.
/// Actúa únicamente con el permiso `etiquetar` (mínimo privilegio): no hay
/// forma de que un dispositivo pida otro permiso, está hardcodeado acá.
#[derive(Clone)]
pub struct DispositivoActual {
    pub id: Uuid,
    pub device_id: String,
}

impl DispositivoActual {
    pub fn exigir(&self, permiso: &str) -> Result<(), ErrorApi> {
        if permiso == permisos::ETIQUETAR {
            Ok(())
        } else {
            Err(ErrorApi::Prohibido(permiso.to_string()))
        }
    }
}

impl<S> FromRequestParts<S> for DispositivoActual
where
    S: Send + Sync,
{
    type Rejection = ErrorApi;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<DispositivoActual>()
            .cloned()
            .ok_or(ErrorApi::NoAutenticado)
    }
}

fn encabezado<'h>(parts: &'h Parts, nombre: &str) -> Option<&'h str> {
    parts.headers.get(nombre).and_then(|v| v.to_str().ok())
}

/// Middleware de las rutas /etiquetado/*: verifica X-Device-Id, X-Timestamp
/// y X-Signature contra el contrato del firmware. Cualquier fallo (header
/// faltante, dispositivo inexistente o inactivo, timestamp fuera de rango,
/// firma inválida) responde 401 sin detalle del motivo — no ayudar a un
/// atacante a distinguir qué falló.
pub async fn verificar_hmac(
    State(estado): State<Estado>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorApi> {
    let (mut parts, body) = req.into_parts();

    let device_id = encabezado(&parts, "x-device-id")
        .ok_or(ErrorApi::NoAutenticado)?
        .to_string();
    let timestamp_str = encabezado(&parts, "x-timestamp")
        .ok_or(ErrorApi::NoAutenticado)?
        .to_string();
    let firma_recibida = encabezado(&parts, "x-signature")
        .ok_or(ErrorApi::NoAutenticado)?
        .to_string();

    let timestamp: i64 = timestamp_str.parse().map_err(|_| ErrorApi::NoAutenticado)?;
    if (chrono::Utc::now().timestamp() - timestamp).abs() > VENTANA_TIMESTAMP_SEGUNDOS {
        return Err(ErrorApi::NoAutenticado);
    }

    let dispositivo = buscar_activo(&estado.pool, &device_id)
        .await?
        .ok_or(ErrorApi::NoAutenticado)?;

    let bytes = axum::body::to_bytes(body, LIMITE_BODY_BYTES)
        .await
        .map_err(|_| ErrorApi::NoAutenticado)?;
    let cuerpo = std::str::from_utf8(&bytes).map_err(|_| ErrorApi::NoAutenticado)?;

    // Cadena canónica: MÉTODO + "\n" + RUTA + "\n" + TIMESTAMP + "\n" + BODY.
    // RUTA es el path completo tal como lo firma el firmware (p. ej.
    // "/etiquetado/escanear"), sin host ni query. `parts.uri` NO sirve acá:
    // dentro de un `.nest()`, axum lo reescribe al path relativo al
    // sub-router ("/escanear"). `OriginalUri` es la extensión que axum
    // inserta con el path completo pre-nesting.
    let ruta = parts
        .extensions
        .get::<OriginalUri>()
        .map(|o| o.0.path())
        .unwrap_or_else(|| parts.uri.path());
    let canonica = format!(
        "{}\n{}\n{}\n{}",
        parts.method.as_str(),
        ruta,
        timestamp_str,
        cuerpo,
    );

    if !firma_valida(&dispositivo.secreto_hmac, &canonica, &firma_recibida) {
        return Err(ErrorApi::NoAutenticado);
    }

    parts.extensions.insert(DispositivoActual {
        id: dispositivo.id,
        device_id: dispositivo.device_id,
    });

    let req = Request::from_parts(parts, axum::body::Body::from(bytes));
    Ok(next.run(req).await)
}
