pub mod precio;
pub mod rutas;

use serde::{Deserialize, Serialize};

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "compras.estado_recepcion", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EstadoRecepcion {
    Borrador,
    Confirmada,
    Completada,
}
