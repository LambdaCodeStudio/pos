pub mod rutas;

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "clientes.tipo_movimiento_cuenta", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TipoMovimientoCuenta {
    Cargo,
    Pago,
    Ajuste,
}

/// Un renglón de producto de una venta fiada, para descomponer el cargo en
/// `clientes.cargo_items` (fase 5: fiado indexado a producto).
pub struct ItemFiado {
    pub producto_id: Uuid,
    pub producto_nombre: String,
    pub cantidad: Decimal,
}

/// Inserta el cargo de una venta pagada (total) con cuenta corriente, en la
/// MISMA transacción de la sincronización. El límite de crédito SÍ bloquea:
/// excederlo requiere `exceder_limite_credito`. El fiado es todo-o-nada por
/// venta (lo exige `sincronizar_venta`): así cada ítem de la venta es,
/// inequívocamente, un renglón pendiente en `cargo_items`.
///
/// `factor_descuento` es `total_centavos / Σ subtotales` de la venta (1 si no
/// hubo descuento de ticket): queda grabado por renglón para que el FIFO y el
/// reprecio automático valúen lo pendiente sobre el precio corriente SIN
/// perder el descuento con el que se fió (ver `aplicar_reduccion_fifo` y
/// `reindexar_precio_producto`).
pub async fn registrar_cargo_de_venta(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    venta_id: Uuid,
    monto_centavos: i64,
    factor_descuento: Decimal,
    items: &[ItemFiado],
    usuario: &UsuarioActual,
) -> Result<(), ErrorApi> {
    debug_assert!(monto_centavos > 0);
    debug_assert!(factor_descuento > Decimal::ZERO && factor_descuento <= Decimal::ONE);

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

    let movimiento_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO clientes.cuenta_movimientos
            (id, cliente_id, tipo, monto_centavos, venta_id, usuario_id)
        VALUES ($1, $2, 'cargo', $3, $4, $5)
        "#,
        movimiento_id,
        cliente_id,
        monto_centavos,
        venta_id,
        usuario.id,
    )
    .execute(&mut **tx)
    .await?;

    for item in items {
        sqlx::query!(
            r#"
            INSERT INTO clientes.cargo_items
                (id, movimiento_id, cliente_id, producto_id, producto_nombre, cantidad, cantidad_pendiente, factor_descuento)
            VALUES ($1, $2, $3, $4, $5, $6, $6, $7)
            "#,
            Uuid::now_v7(),
            movimiento_id,
            cliente_id,
            item.producto_id,
            item.producto_nombre,
            item.cantidad,
            factor_descuento,
        )
        .execute(&mut **tx)
        .await?;
    }

    actualizar_saldo(tx, cliente_id, monto_centavos).await
}

/// Contra-asiento del cargo de una venta anulada (tipo ajuste, monto
/// invertido, referenciando la venta). El llamador garantiza idempotencia
/// vía el estado de la venta. Los renglones de `cargo_items` del cargo
/// revertido dejan de estar pendientes: la venta anulada no debe seguir
/// consumiendo pagos por FIFO ni revalorizándose si el producto cambia de
/// precio.
pub async fn revertir_cargos_de_venta(
    tx: &mut Transaction<'_, Postgres>,
    venta_id: Uuid,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    let cargos = sqlx::query!(
        r#"SELECT id, cliente_id, monto_centavos FROM clientes.cuenta_movimientos
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

        sqlx::query!(
            r#"UPDATE clientes.cargo_items SET cantidad_pendiente = 0 WHERE movimiento_id = $1"#,
            cargo.id,
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

fn redondear_centavos(valor: Decimal) -> i64 {
    valor
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

/// Consume, a precio corriente, los renglones de cargo pendientes de un
/// cliente por orden de antigüedad (FIFO): el pago (o condonación) más
/// nuevo salda primero la compra fiada más vieja. Lo que sobra tras saldar
/// todo lo pendiente queda como crédito a favor del cliente (mismo
/// comportamiento que hoy: el saldo puede ir a negativo). `monto_centavos`
/// es SIEMPRE positivo: el monto que reduce deuda, sea un pago o el valor
/// absoluto de un ajuste negativo (condonación).
async fn aplicar_reduccion_fifo(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    movimiento_id: Uuid,
    mut monto_centavos: i64,
) -> Result<(), ErrorApi> {
    if monto_centavos <= 0 {
        return Ok(());
    }

    let pendientes = sqlx::query!(
        r#"
        SELECT ci.id, ci.cantidad_pendiente, ci.factor_descuento, p.precio_actual_centavos
        FROM clientes.cargo_items ci
        JOIN catalogo.productos p ON p.id = ci.producto_id
        WHERE ci.cliente_id = $1 AND ci.cantidad_pendiente > 0
        ORDER BY ci.creado_en
        FOR UPDATE OF ci
        "#,
        cliente_id,
    )
    .fetch_all(&mut **tx)
    .await?;

    for item in pendientes {
        if monto_centavos <= 0 {
            break;
        }
        let precio = Decimal::from(item.precio_actual_centavos.unwrap_or(0));
        if precio <= Decimal::ZERO {
            continue;
        }
        // Precio corriente ponderado por el descuento de ticket con el que
        // se fió este renglón (1 si no hubo descuento).
        let precio_neto = precio * item.factor_descuento;

        let valor_pendiente_centavos = redondear_centavos(item.cantidad_pendiente * precio_neto);
        if valor_pendiente_centavos <= 0 {
            continue;
        }

        let (cantidad_aplicada, valor_aplicado_centavos, nueva_pendiente) =
            if monto_centavos >= valor_pendiente_centavos {
                (item.cantidad_pendiente, valor_pendiente_centavos, Decimal::ZERO)
            } else {
                let cantidad_aplicada = (Decimal::from(monto_centavos) / precio_neto)
                    .min(item.cantidad_pendiente);
                (
                    cantidad_aplicada,
                    monto_centavos,
                    item.cantidad_pendiente - cantidad_aplicada,
                )
            };

        if cantidad_aplicada <= Decimal::ZERO {
            continue;
        }

        sqlx::query!(
            r#"UPDATE clientes.cargo_items SET cantidad_pendiente = $2 WHERE id = $1"#,
            item.id,
            nueva_pendiente,
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO clientes.cargo_aplicaciones
                (id, pago_movimiento_id, cargo_item_id, cantidad_aplicada, valor_centavos_aplicado)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            Uuid::now_v7(),
            movimiento_id,
            item.id,
            cantidad_aplicada,
            valor_aplicado_centavos,
        )
        .execute(&mut **tx)
        .await?;

        monto_centavos -= valor_aplicado_centavos;
    }

    Ok(())
}

/// Consume por FIFO los renglones pendientes de un cliente cuando registra
/// un pago sobre su cuenta.
pub async fn aplicar_pago_fifo(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    pago_movimiento_id: Uuid,
    monto_centavos: i64,
) -> Result<(), ErrorApi> {
    aplicar_reduccion_fifo(tx, cliente_id, pago_movimiento_id, monto_centavos).await
}

/// Consume por FIFO los renglones pendientes de un cliente cuando se le
/// condona parte de la deuda (ajuste negativo).
pub async fn aplicar_condonacion_fifo(
    tx: &mut Transaction<'_, Postgres>,
    cliente_id: Uuid,
    ajuste_movimiento_id: Uuid,
    monto_condonado_centavos: i64,
) -> Result<(), ErrorApi> {
    aplicar_reduccion_fifo(tx, cliente_id, ajuste_movimiento_id, monto_condonado_centavos).await
}

/// Revalúa a precio corriente los renglones de cargo pendientes de un
/// producto cuando su precio cambia —para arriba o para abajo—, y asienta
/// la diferencia como un ajuste automático en la cuenta de cada cliente
/// afectado. Llamar en la MISMA transacción que actualiza
/// `catalogo.productos.precio_actual_centavos`, con el precio anterior
/// (antes del UPDATE) y el nuevo.
pub async fn reindexar_precio_producto(
    tx: &mut Transaction<'_, Postgres>,
    producto_id: Uuid,
    precio_anterior_centavos: Option<i64>,
    precio_nuevo_centavos: i64,
    usuario_id: Uuid,
) -> Result<(), ErrorApi> {
    let precio_anterior = precio_anterior_centavos.unwrap_or(0);
    if precio_anterior == precio_nuevo_centavos {
        return Ok(());
    }
    let delta_unitario = Decimal::from(precio_nuevo_centavos - precio_anterior);

    // Sin agregar en SQL: renglones del mismo producto pueden venir de
    // ventas con distinto descuento de ticket (factor_descuento propio), así
    // que el delta se pondera por renglón antes de sumar por cliente.
    let pendientes = sqlx::query!(
        r#"
        SELECT cliente_id, cantidad_pendiente, factor_descuento
        FROM clientes.cargo_items
        WHERE producto_id = $1 AND cantidad_pendiente > 0
        "#,
        producto_id,
    )
    .fetch_all(&mut **tx)
    .await?;
    if pendientes.is_empty() {
        return Ok(());
    }

    let mut deltas_por_cliente: HashMap<Uuid, i64> = HashMap::new();
    for fila in &pendientes {
        let delta = redondear_centavos(fila.cantidad_pendiente * delta_unitario * fila.factor_descuento);
        if delta == 0 {
            continue;
        }
        *deltas_por_cliente.entry(fila.cliente_id).or_insert(0) += delta;
    }
    if deltas_por_cliente.is_empty() {
        return Ok(());
    }

    // Nombre snapshoteado en el motivo: legible en la cuenta del cliente sin
    // tener que resolver el UUID.
    let producto_nombre = sqlx::query_scalar!(
        r#"SELECT nombre FROM catalogo.productos WHERE id = $1"#,
        producto_id,
    )
    .fetch_one(&mut **tx)
    .await?;

    for (cliente_id, delta) in deltas_por_cliente {
        sqlx::query!(
            r#"
            INSERT INTO clientes.cuenta_movimientos
                (id, cliente_id, tipo, monto_centavos, motivo, usuario_id)
            VALUES ($1, $2, 'ajuste', $3, $4, $5)
            "#,
            Uuid::now_v7(),
            cliente_id,
            delta,
            format!("cambio de precio: {producto_nombre}"),
            usuario_id,
        )
        .execute(&mut **tx)
        .await?;

        actualizar_saldo(tx, cliente_id, delta).await?;
    }

    Ok(())
}
