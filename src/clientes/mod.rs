pub mod rutas;

use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "tipo_movimiento_cuenta", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TipoMovimientoCuenta {
    Cargo,
    Pago,
    Ajuste,
}

/// Inserta el cargo de una venta pagada (total o parcialmente) con cuenta
/// corriente, en la MISMA transacción de la sincronización. El límite de
/// crédito SÍ bloquea: excederlo requiere `exceder_limite_credito`.
pub async fn registrar_cargo_de_venta(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    venta_id: Uuid,
    monto_centavos: i64,
    usuario: &UsuarioActual,
) -> Result<(), ErrorApi> {
    debug_assert!(monto_centavos > 0);

    let cliente = sqlx::query!(
        r#"SELECT saldo_actual_centavos, limite_credito_centavos, activo
           FROM clientes.clientes WHERE id = $1 FOR UPDATE"#,
        cliente_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ErrorApi::Validacion("cliente inexistente".into()))?;

    if !cliente.activo {
        return Err(ErrorApi::Validacion("el cliente está inactivo".into()));
    }

    let nuevo_saldo = cliente.saldo_actual_centavos + monto_centavos;
    if let Some(limite) = cliente.limite_credito_centavos {
        if nuevo_saldo > limite {
            usuario.exigir(permisos::EXCEDER_LIMITE_CREDITO)?;
        }
    }

    sqlx::query!(
        r#"
        INSERT INTO clientes.cuenta_movimientos
            (id, cliente_id, tipo, monto_centavos, venta_id, usuario_id)
        VALUES ($1, $2, 'cargo', $3, $4, $5)
        "#,
        Uuid::now_v7(),
        cliente_id,
        monto_centavos,
        venta_id,
        usuario.id,
    )
    .execute(&mut **tx)
    .await?;

    actualizar_saldo(tx, cliente_id, monto_centavos).await
}

/// Contra-asiento del cargo de una venta anulada (tipo ajuste, monto
/// invertido, referenciando la venta). El llamador garantiza idempotencia
/// vía el estado de la venta.
pub async fn revertir_cargos_de_venta(
    tx: &mut Transaction<'_, Postgres>,
    venta_id: Uuid,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    let cargos = sqlx::query!(
        r#"SELECT cliente_id, monto_centavos FROM clientes.cuenta_movimientos
           WHERE venta_id = $1 AND tipo = 'cargo'"#,
        venta_id,
    )
    .fetch_all(&mut **tx)
    .await?;

    for cargo in cargos {
        sqlx::query!(
            r#"
            INSERT INTO clientes.cuenta_movimientos
                (id, cliente_id, tipo, monto_centavos, venta_id, motivo, usuario_id)
            VALUES ($1, $2, 'ajuste', $3, $4, 'anulacion_venta', $5)
            "#,
            Uuid::now_v7(),
            cargo.cliente_id,
            -cargo.monto_centavos,
            venta_id,
            usuario_id,
        )
        .execute(&mut **tx)
        .await?;

        actualizar_saldo(tx, cargo.cliente_id, -cargo.monto_centavos).await?;
    }
    Ok(())
}

/// Aplica un delta a la proyección de saldo. Debe poder reconstruirse con
/// SUM(monto_centavos) del ledger.
pub async fn actualizar_saldo(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    delta_centavos: i64,
) -> Result<(), ErrorApi> {
    sqlx::query!(
        r#"UPDATE clientes.clientes
           SET saldo_actual_centavos = saldo_actual_centavos + $2, actualizado_en = now()
           WHERE id = $1"#,
        cliente_id,
        delta_centavos,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}
