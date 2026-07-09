pub mod rutas;

use serde::{Deserialize, Serialize};

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "estado_venta", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EstadoVenta {
    Confirmada,
    Anulada,
}

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "medio_pago", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MedioPago {
    Efectivo,
    Tarjeta,
    MercadoPago,
    Transferencia,
    CuentaCorriente,
}
