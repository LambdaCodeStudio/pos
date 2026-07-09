use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::estado::Estado;

const HORAS_VALIDEZ_TOKEN: i64 = 12;

pub fn hashear_secreto(secreto: &str) -> Result<String, ErrorApi> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secreto.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ErrorApi::Interno(format!("error al hashear: {e}")))
}

pub fn verificar_secreto(hash: &str, secreto: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| {
            Argon2::default()
                .verify_password(secreto.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

/// PIN corto para cambio rápido de operador en la caja compartida.
pub fn validar_formato_pin(pin: &str) -> Result<(), ErrorApi> {
    if pin.len() < 4 || pin.len() > 6 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ErrorApi::Validacion(
            "el PIN debe tener entre 4 y 6 dígitos".into(),
        ));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: i64,
}

pub fn emitir_token(usuario_id: Uuid, jwt_secret: &str) -> Result<String, ErrorApi> {
    let claims = Claims {
        sub: usuario_id,
        exp: (chrono::Utc::now() + chrono::Duration::hours(HORAS_VALIDEZ_TOKEN)).timestamp(),
    };
    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| ErrorApi::Interno(format!("error al emitir token: {e}")))
}

fn decodificar_token(token: &str, jwt_secret: &str) -> Result<Claims, ErrorApi> {
    jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|_| ErrorApi::NoAutenticado)
}

/// Usuario autenticado del request. Los permisos se cargan de la base en cada
/// request (rol + individuales aditivos): un cambio de rol o desactivación
/// tiene efecto inmediato, sin esperar a que venza el token.
pub struct UsuarioActual {
    pub id: Uuid,
    pub nombre: String,
    pub permisos: HashSet<String>,
}

impl UsuarioActual {
    /// Denegado por defecto: si el permiso no está en el conjunto, 403.
    pub fn exigir(&self, permiso: &str) -> Result<(), ErrorApi> {
        if self.permisos.contains(permiso) {
            Ok(())
        } else {
            Err(ErrorApi::Prohibido(permiso.to_string()))
        }
    }
}

impl FromRequestParts<Estado> for UsuarioActual {
    type Rejection = ErrorApi;

    async fn from_request_parts(
        parts: &mut Parts,
        estado: &Estado,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ErrorApi::NoAutenticado)?;

        let claims = decodificar_token(token, &estado.jwt_secret)?;
        cargar_usuario_con_permisos(&estado.pool, claims.sub).await
    }
}

pub async fn cargar_usuario_con_permisos(
    pool: &sqlx::PgPool,
    usuario_id: Uuid,
) -> Result<UsuarioActual, ErrorApi> {
    let usuario = sqlx::query!(
        r#"SELECT id, nombre, activo FROM identidad.usuarios WHERE id = $1"#,
        usuario_id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(ErrorApi::NoAutenticado)?;

    if !usuario.activo {
        return Err(ErrorApi::NoAutenticado);
    }

    let permisos: HashSet<String> = sqlx::query_scalar!(
        r#"
        SELECT rp.permiso AS "permiso!"
        FROM identidad.usuarios u
        JOIN identidad.roles r ON r.id = u.rol_id AND r.activo
        JOIN identidad.rol_permisos rp ON rp.rol_id = r.id
        WHERE u.id = $1
        UNION
        SELECT up.permiso
        FROM identidad.usuario_permisos up
        WHERE up.usuario_id = $1
        "#,
        usuario_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    Ok(UsuarioActual {
        id: usuario.id,
        nombre: usuario.nombre,
        permisos,
    })
}
