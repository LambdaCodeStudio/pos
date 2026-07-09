//! Endpoints de consulta para métricas. NO es un motor de reportes (excluido
//! por diseño): son agregaciones simples de solo lectura sobre los ledgers y
//! documentos, bajo el permiso `ver_reportes`.

pub mod rutas;

/// Los días contables del negocio se cortan en hora local del local.
pub const ZONA_HORARIA: &str = "America/Argentina/Buenos_Aires";
