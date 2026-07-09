//! Siembra del primer usuario administrador. Sin esto, un sistema con
//! denegado-por-defecto y sin usuarios sería inoperable.

use sqlx::PgPool;
use uuid::Uuid;

use crate::identidad::auth::hashear_secreto;

const ROL_ADMINISTRADOR_ID: Uuid = Uuid::from_u128(0x01900000_0000_7000_8000_000000000001);

pub async fn sembrar_admin_si_no_hay_usuarios(
    pool: &PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let hay_usuarios = sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM identidad.usuarios) AS "existe!""#)
        .fetch_one(pool)
        .await?;
    if hay_usuarios {
        return Ok(());
    }

    let password = std::env::var("ADMIN_PASSWORD_INICIAL").unwrap_or_else(|_| "admin1234".into());
    let hash = hashear_secreto(&password)
        .map_err(|e| format!("no se pudo hashear la contraseña inicial: {e}"))?;
    let admin_id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO identidad.usuarios (id, nombre, password_hash, rol_id)
        VALUES ($1, 'admin', $2, $3)
        "#,
        admin_id,
        hash,
        ROL_ADMINISTRADOR_ID,
    )
    .execute(pool)
    .await?;

    crate::auditoria::registrar(
        pool,
        "usuario",
        Some(admin_id),
        "bootstrap_admin",
        None,
        None,
    )
    .await?;

    tracing::warn!("usuario 'admin' creado con la contraseña inicial — cambiala cuanto antes");
    Ok(())
}
