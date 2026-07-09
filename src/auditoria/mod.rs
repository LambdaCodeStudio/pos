//! Auditoría transversal. Registra SOLO mutaciones de datos maestros y
//! acciones de seguridad — los ledgers ya auditan los hechos de negocio.

pub mod rutas;

use serde_json::Value;
use uuid::Uuid;

/// Registra un evento de auditoría dentro de la transacción/conexión dada,
/// para que el evento sea atómico con la mutación que lo origina.
pub async fn registrar<'e, E>(
    ejecutor: E,
    entidad: &str,
    entidad_id: Option<Uuid>,
    accion: &str,
    usuario_id: Option<Uuid>,
    diff: Option<Value>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query!(
        r#"
        INSERT INTO auditoria.auditoria_eventos (id, entidad, entidad_id, accion, usuario_id, diff)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        Uuid::now_v7(),
        entidad,
        entidad_id,
        accion,
        usuario_id,
        diff,
    )
    .execute(ejecutor)
    .await?;
    Ok(())
}

/// Arma un diff JSONB {"antes": ..., "despues": ...} para eventos de edición.
pub fn diff_antes_despues(antes: Value, despues: Value) -> Value {
    serde_json::json!({ "antes": antes, "despues": despues })
}
