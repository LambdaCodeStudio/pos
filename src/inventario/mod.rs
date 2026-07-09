//! Contexto INVENTARIO: ledger de movimientos (solo-INSERT), lotes para
//! alertas de vencimiento accionables, proyección de stock y documento de
//! ajustes. El stock es literalmente SUM(cantidad) del ledger.

pub mod rutas;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::error::ErrorApi;

/// Depósito "Principal" sembrado en la migración. Único depósito de hoy;
/// las features multi-depósito no se construyen.
pub const DEPOSITO_PRINCIPAL_ID: Uuid = Uuid::from_u128(0x01900000_0000_7000_8000_00000000d001);

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "tipo_movimiento", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TipoMovimiento {
    EntradaRecepcion,
    SalidaVenta,
    DevolucionCliente,
    DevolucionProveedor,
    Ajuste,
}

/// Motivo del documento de ajuste. Robo, pérdida y vencimiento son motivos,
/// no tipos de movimiento: el robo se descubre como faltante en un conteo.
#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "motivo_ajuste", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MotivoAjuste {
    Perdida,
    Rotura,
    Vencimiento,
    Robo,
    Conteo,
    Otro,
}

/// Registra la entrada de mercadería de un ítem de recepción confirmado:
/// crea el lote si el producto controla vencimiento, inserta el movimiento
/// en el ledger (solo-INSERT) y actualiza la proyección de stock — todo
/// dentro de la transacción de confirmación del llamador.
pub async fn registrar_entrada_recepcion(
    tx: &mut Transaction<'_, Postgres>,
    producto_id: Uuid,
    cantidad: Decimal,
    recepcion_item_id: Uuid,
    vencimiento: Option<NaiveDate>,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    let lote_id = match vencimiento {
        Some(fecha) => {
            let lote_id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO inventario.lotes
                    (id, producto_id, vencimiento, recepcion_item_id, cantidad_actual)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                lote_id,
                producto_id,
                fecha,
                recepcion_item_id,
                cantidad,
            )
            .execute(&mut **tx)
            .await?;
            Some(lote_id)
        }
        None => None,
    };

    sqlx::query!(
        r#"
        INSERT INTO inventario.movimientos_stock
            (id, producto_id, deposito_id, lote_id, cantidad, tipo, recepcion_item_id, usuario_id)
        VALUES ($1, $2, $3, $4, $5, 'entrada_recepcion', $6, $7)
        "#,
        Uuid::now_v7(),
        producto_id,
        DEPOSITO_PRINCIPAL_ID,
        lote_id,
        cantidad,
        recepcion_item_id,
        usuario_id,
    )
    .execute(&mut **tx)
    .await?;

    actualizar_stock_actual(&mut *tx, producto_id, DEPOSITO_PRINCIPAL_ID, cantidad).await?;
    Ok(())
}

/// Salida de mercadería por venta sincronizada, con FEFO por asunción:
/// descuenta del lote con vencimiento más próximo que tenga stock, en cascada
/// si la cantidad abarca varios lotes; lo que no alcanza a cubrirse con lotes
/// sale sin lote. NUNCA se pide selección de lote en la caja: es una
/// aproximación deliberada que el conteo físico recalibra vía ajuste.
///
/// El stock del depósito SÍ puede quedar negativo (la caja jamás bloquea con
/// el cliente en el mostrador); los lotes, en cambio, nunca bajan de cero.
pub async fn registrar_salida_venta_fefo(
    tx: &mut Transaction<'_, Postgres>,
    producto_id: Uuid,
    cantidad: Decimal,
    venta_item_id: Uuid,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    debug_assert!(cantidad > Decimal::ZERO);

    let lotes = sqlx::query!(
        r#"
        SELECT id, cantidad_actual FROM inventario.lotes
        WHERE producto_id = $1 AND cantidad_actual > 0
        ORDER BY vencimiento, creado_en
        FOR UPDATE
        "#,
        producto_id,
    )
    .fetch_all(&mut **tx)
    .await?;

    let mut restante = cantidad;
    for lote in lotes {
        if restante <= Decimal::ZERO {
            break;
        }
        let tomar = restante.min(lote.cantidad_actual);
        sqlx::query!(
            r#"
            INSERT INTO inventario.movimientos_stock
                (id, producto_id, deposito_id, lote_id, cantidad, tipo, venta_item_id, usuario_id)
            VALUES ($1, $2, $3, $4, $5, 'salida_venta', $6, $7)
            "#,
            Uuid::now_v7(),
            producto_id,
            DEPOSITO_PRINCIPAL_ID,
            lote.id,
            -tomar,
            venta_item_id,
            usuario_id,
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query!(
            r#"UPDATE inventario.lotes SET cantidad_actual = cantidad_actual - $2 WHERE id = $1"#,
            lote.id,
            tomar,
        )
        .execute(&mut **tx)
        .await?;

        restante -= tomar;
    }

    // Resto sin lote: productos sin lotes, o venta que excede lo loteado.
    if restante > Decimal::ZERO {
        sqlx::query!(
            r#"
            INSERT INTO inventario.movimientos_stock
                (id, producto_id, deposito_id, lote_id, cantidad, tipo, venta_item_id, usuario_id)
            VALUES ($1, $2, $3, NULL, $4, 'salida_venta', $5, $6)
            "#,
            Uuid::now_v7(),
            producto_id,
            DEPOSITO_PRINCIPAL_ID,
            -restante,
            venta_item_id,
            usuario_id,
        )
        .execute(&mut **tx)
        .await?;
    }

    actualizar_stock_actual(&mut *tx, producto_id, DEPOSITO_PRINCIPAL_ID, -cantidad).await
}

/// Reversa de los movimientos de un ítem de venta anulada: contra-asientos
/// con signo invertido referenciando el mismo ítem y los mismos lotes.
/// Jamás UPDATE/DELETE sobre el ledger. El llamador garantiza que la venta
/// pasa a `anulada` en la misma transacción (eso da la idempotencia).
pub async fn registrar_reversa_venta(
    tx: &mut Transaction<'_, Postgres>,
    venta_item_id: Uuid,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    let originales = sqlx::query!(
        r#"
        SELECT producto_id, deposito_id, lote_id, cantidad
        FROM inventario.movimientos_stock
        WHERE venta_item_id = $1 AND tipo = 'salida_venta' AND cantidad < 0
        "#,
        venta_item_id,
    )
    .fetch_all(&mut **tx)
    .await?;

    for mov in originales {
        let reverso = -mov.cantidad;
        sqlx::query!(
            r#"
            INSERT INTO inventario.movimientos_stock
                (id, producto_id, deposito_id, lote_id, cantidad, tipo, venta_item_id, usuario_id)
            VALUES ($1, $2, $3, $4, $5, 'salida_venta', $6, $7)
            "#,
            Uuid::now_v7(),
            mov.producto_id,
            mov.deposito_id,
            mov.lote_id,
            reverso,
            venta_item_id,
            usuario_id,
        )
        .execute(&mut **tx)
        .await?;

        if let Some(lote_id) = mov.lote_id {
            sqlx::query!(
                r#"UPDATE inventario.lotes SET cantidad_actual = cantidad_actual + $2 WHERE id = $1"#,
                lote_id,
                reverso,
            )
            .execute(&mut **tx)
            .await?;
        }

        actualizar_stock_actual(&mut *tx, mov.producto_id, mov.deposito_id, reverso).await?;
    }

    Ok(())
}

/// Aplica un delta a la proyección stock_actual (upsert). El ledger es la
/// fuente de verdad: esta proyección debe poder reconstruirse con
/// SUM(cantidad) de movimientos_stock.
pub async fn actualizar_stock_actual(
    conn: &mut PgConnection,
    producto_id: Uuid,
    deposito_id: Uuid,
    delta: Decimal,
) -> Result<(), ErrorApi> {
    sqlx::query!(
        r#"
        INSERT INTO inventario.stock_actual (producto_id, deposito_id, cantidad)
        VALUES ($1, $2, $3)
        ON CONFLICT (producto_id, deposito_id)
        DO UPDATE SET cantidad = inventario.stock_actual.cantidad + EXCLUDED.cantidad,
                      actualizado_en = now()
        "#,
        producto_id,
        deposito_id,
        delta,
    )
    .execute(conn)
    .await?;
    Ok(())
}
