pub mod rutas;

use serde::{Deserialize, Serialize};

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "ventas.estado_venta", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EstadoVenta {
    Confirmada,
    Anulada,
}

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "ventas.medio_pago", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MedioPago {
    Efectivo,
    Tarjeta,
    MercadoPago,
    Transferencia,
    CuentaCorriente,
}
