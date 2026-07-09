use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ErrorApi {
    #[error("no autenticado")]
    NoAutenticado,
    #[error("permiso denegado: {0}")]
    Prohibido(String),
    #[error("no encontrado")]
    NoEncontrado,
    #[error("{0}")]
    Validacion(String),
    #[error("{0}")]
    Conflicto(String),
    #[error("error interno")]
    Interno(String),
}

impl From<sqlx::Error> for ErrorApi {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ErrorApi::NoEncontrado,
            otro => ErrorApi::Interno(otro.to_string()),
        }
    }
}

impl IntoResponse for ErrorApi {
    fn into_response(self) -> Response {
        let (status, mensaje) = match &self {
            ErrorApi::NoAutenticado => (StatusCode::UNAUTHORIZED, self.to_string()),
            ErrorApi::Prohibido(_) => (StatusCode::FORBIDDEN, self.to_string()),
            ErrorApi::NoEncontrado => (StatusCode::NOT_FOUND, self.to_string()),
            ErrorApi::Validacion(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            ErrorApi::Conflicto(_) => (StatusCode::CONFLICT, self.to_string()),
            ErrorApi::Interno(detalle) => {
                tracing::error!(error = %detalle, "error interno");
                (StatusCode::INTERNAL_SERVER_ERROR, "error interno".into())
            }
        };
        (status, Json(json!({ "error": mensaje }))).into_response()
    }
}
