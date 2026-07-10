use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auditoria;
use crate::clientes::{actualizar_saldo, TipoMovimientoCuenta};
use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;
use crate::ventas::MedioPago;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/", get(listar_clientes).post(crear_cliente))
        .route(
            "/{id}",
            get(obtener_cliente)
                .patch(actualizar_cliente)
                .delete(desactivar_cliente),
        )
        .route("/{id}/cuenta", get(movimientos_de_cuenta))
        .route("/{id}/pagos", post(registrar_pago))
        .route("/{id}/ajustes", post(registrar_ajuste))
}

// ---------- Maestro de clientes ----------

#[derive(Serialize)]
struct Cliente {
    id: Uuid,
    nombre: String,
    telefono: Option<String>,
    documento: Option<String>,
    limite_credito_centavos: Option<i64>,
    saldo_actual_centavos: i64,
    activo: bool,
}

#[derive(Deserialize)]
struct FiltroClientes {
    buscar: Option<String>,
    incluir_inactivos: Option<bool>,
    /// Solo clientes con saldo distinto de cero (la libreta pendiente).
    con_saldo: Option<bool>,
    limite: Option<i64>,
}

async fn listar_clientes(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroClientes>,
) -> Result<Json<Vec<Cliente>>, ErrorApi> {
    let incluir_inactivos = filtro.incluir_inactivos.unwrap_or(false);
    let con_saldo = filtro.con_saldo.unwrap_or(false);
    let limite = filtro.limite.unwrap_or(50).clamp(1, 200);
    let buscar = filtro.buscar.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let filas = sqlx::query_as!(
        Cliente,
        r#"
        SELECT id, nombre, telefono, documento, limite_credito_centavos,
               saldo_actual_centavos, activo
        FROM clientes.clientes
        WHERE (activo OR $1)
          AND ($2::text IS NULL OR nombre ILIKE '%' || $2 || '%')
          AND (NOT $3 OR saldo_actual_centavos <> 0)
        ORDER BY nombre
        LIMIT $4
        "#,
        incluir_inactivos,
        buscar,
        con_saldo,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;
    Ok(Json(filas))
}

async fn obtener_cliente(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<Cliente>, ErrorApi> {
    let cliente = sqlx::query_as!(
        Cliente,
        r#"
        SELECT id, nombre, telefono, documento, limite_credito_centavos,
               saldo_actual_centavos, activo
        FROM clientes.clientes WHERE id = $1
        "#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;
    Ok(Json(cliente))
}

#[derive(Deserialize)]
struct CrearCliente {
    nombre: String,
    telefono: Option<String>,
    documento: Option<String>,
    limite_credito_centavos: Option<i64>,
}

async fn crear_cliente(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearCliente>,
) -> Result<Json<Cliente>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CLIENTES)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }
    if datos.limite_credito_centavos.is_some_and(|l| l < 0) {
        return Err(ErrorApi::Validacion("el límite no puede ser negativo".into()));
    }

    let id = Uuid::now_v7();
    let mut tx = estado.pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO clientes.clientes (id, nombre, telefono, documento, limite_credito_centavos)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        id,
        datos.nombre.trim(),
        datos.telefono,
        datos.documento,
        datos.limite_credito_centavos,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "cliente",
        Some(id),
        "crear",
        Some(usuario.id),
        Some(json!({
            "nombre": datos.nombre.trim(),
            "limite_credito_centavos": datos.limite_credito_centavos,
        })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(Cliente {
        id,
        nombre: datos.nombre.trim().to_string(),
        telefono: datos.telefono,
        documento: datos.documento,
        limite_credito_centavos: datos.limite_credito_centavos,
        saldo_actual_centavos: 0,
        activo: true,
    }))
}

#[derive(Deserialize)]
struct ActualizarCliente {
    nombre: Option<String>,
    telefono: Option<String>,
    documento: Option<String>,
    /// Doble Option: ausente = no tocar, null = quitar el límite.
    #[serde(default)]
    limite_credito_centavos: Option<Option<i64>>,
    activo: Option<bool>,
}

async fn actualizar_cliente(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarCliente>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CLIENTES)?;
    if let Some(Some(l)) = datos.limite_credito_centavos {
        if l < 0 {
            return Err(ErrorApi::Validacion("el límite no puede ser negativo".into()));
        }
    }

    let mut tx = estado.pool.begin().await?;
    let antes = sqlx::query!(
        r#"SELECT nombre, telefono, documento, limite_credito_centavos, activo
           FROM clientes.clientes WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let (cambiar_limite, nuevo_limite) = match datos.limite_credito_centavos {
        Some(v) => (true, v),
        None => (false, None),
    };

    sqlx::query!(
        r#"
        UPDATE clientes.clientes SET
            nombre = COALESCE($2, nombre),
            telefono = COALESCE($3, telefono),
            documento = COALESCE($4, documento),
            limite_credito_centavos = CASE WHEN $5 THEN $6 ELSE limite_credito_centavos END,
            activo = COALESCE($7, activo),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        datos.telefono,
        datos.documento,
        cambiar_limite,
        nuevo_limite,
        datos.activo,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "cliente",
        Some(id),
        "actualizar",
        Some(usuario.id),
        Some(auditoria::diff_antes_despues(
            json!({
                "nombre": antes.nombre, "telefono": antes.telefono,
                "documento": antes.documento,
                "limite_credito_centavos": antes.limite_credito_centavos,
                "activo": antes.activo,
            }),
            json!({
                "nombre": datos.nombre, "telefono": datos.telefono,
                "documento": datos.documento,
                "limite_credito_centavos": datos.limite_credito_centavos,
                "activo": datos.activo,
            }),
        )),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn desactivar_cliente(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CLIENTES)?;
    let mut tx = estado.pool.begin().await?;
    let resultado = sqlx::query!(
        r#"UPDATE clientes.clientes SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;
    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "cliente", Some(id), "desactivar", Some(usuario.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Cuenta corriente ----------

#[derive(Serialize)]
struct MovimientoCuenta {
    id: Uuid,
    tipo: TipoMovimientoCuenta,
    monto_centavos: i64,
    venta_id: Option<Uuid>,
    medio_pago: Option<MedioPago>,
    motivo: Option<String>,
    usuario_id: Uuid,
    creado_en: DateTime<Utc>,
}

async fn movimientos_de_cuenta(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Query(filtro): Query<FiltroLimite>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let cliente = sqlx::query!(
        r#"SELECT nombre, saldo_actual_centavos FROM clientes.clientes WHERE id = $1"#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let limite = filtro.limite.unwrap_or(100).clamp(1, 500);
    let movimientos = sqlx::query_as!(
        MovimientoCuenta,
        r#"
        SELECT id, tipo AS "tipo: TipoMovimientoCuenta", monto_centavos, venta_id,
               medio_pago AS "medio_pago: MedioPago", motivo, usuario_id, creado_en
        FROM clientes.cuenta_movimientos
        WHERE cliente_id = $1
        ORDER BY creado_en DESC
        LIMIT $2
        "#,
        id,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    // Detalle de productos por renglón de cargo (fase 5: fiado indexado a
    // producto), para mostrar qué se fió y cuánto de eso sigue pendiente.
    let cargo_ids: Vec<Uuid> = movimientos
        .iter()
        .filter(|m| m.tipo == TipoMovimientoCuenta::Cargo)
        .map(|m| m.id)
        .collect();
    let renglones = sqlx::query!(
        r#"
        SELECT movimiento_id, producto_id, producto_nombre, cantidad, cantidad_pendiente
        FROM clientes.cargo_items
        WHERE movimiento_id = ANY($1)
        ORDER BY producto_nombre
        "#,
        &cargo_ids,
    )
    .fetch_all(&estado.pool)
    .await?;

    let mut items_por_movimiento: std::collections::HashMap<Uuid, Vec<serde_json::Value>> =
        std::collections::HashMap::new();
    for r in renglones {
        items_por_movimiento.entry(r.movimiento_id).or_default().push(json!({
            "producto_id": r.producto_id,
            "producto_nombre": r.producto_nombre,
            "cantidad": r.cantidad,
            "cantidad_pendiente": r.cantidad_pendiente,
        }));
    }

    let movimientos: Vec<serde_json::Value> = movimientos
        .into_iter()
        .map(|m| {
            let mut valor = serde_json::to_value(&m).expect("MovimientoCuenta serializa");
            valor["items"] = json!(items_por_movimiento.remove(&m.id).unwrap_or_default());
            valor
        })
        .collect();

    Ok(Json(json!({
        "cliente_id": id,
        "cliente_nombre": cliente.nombre,
        "saldo_actual_centavos": cliente.saldo_actual_centavos,
        "movimientos": movimientos,
    })))
}

#[derive(Deserialize)]
struct FiltroLimite {
    limite: Option<i64>,
}

#[derive(Deserialize)]
struct RegistrarPago {
    /// UUID generado por el cliente para idempotencia.
    id: Option<Uuid>,
    monto_centavos: i64,
    medio: MedioPago,
    referencia_externa: Option<String>,
}

/// Registra un pago del cliente sobre su cuenta (saldo global corrido, sin
/// imputación contra ventas específicas).
async fn registrar_pago(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(cliente_id): Path<Uuid>,
    Json(datos): Json<RegistrarPago>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CLIENTES)?;
    if datos.monto_centavos <= 0 {
        return Err(ErrorApi::Validacion("el monto del pago debe ser positivo".into()));
    }
    if datos.medio == MedioPago::CuentaCorriente {
        return Err(ErrorApi::Validacion(
            "la cuenta corriente no se paga con cuenta corriente".into(),
        ));
    }

    let movimiento_id = datos.id.unwrap_or_else(Uuid::now_v7);
    let mut tx = estado.pool.begin().await?;

    let cliente = sqlx::query!(
        r#"SELECT saldo_actual_centavos FROM clientes.clientes WHERE id = $1 FOR UPDATE"#,
        cliente_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    // Idempotencia por UUID del movimiento.
    let insertado = sqlx::query!(
        r#"
        INSERT INTO clientes.cuenta_movimientos
            (id, cliente_id, tipo, monto_centavos, medio_pago, motivo, usuario_id)
        VALUES ($1, $2, 'pago', $3, $4, $5, $6)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        movimiento_id,
        cliente_id,
        -datos.monto_centavos,
        datos.medio as MedioPago,
        datos.referencia_externa,
        usuario.id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    if insertado.is_none() {
        return Ok(Json(json!({ "id": movimiento_id, "ya_estaba_aplicado": true })));
    }

    actualizar_saldo(&mut tx, cliente_id, -datos.monto_centavos).await?;
    crate::clientes::aplicar_pago_fifo(&mut tx, cliente_id, movimiento_id, datos.monto_centavos)
        .await?;
    tx.commit().await?;

    Ok(Json(json!({
        "id": movimiento_id,
        "ya_estaba_aplicado": false,
        "saldo_resultante_centavos": cliente.saldo_actual_centavos - datos.monto_centavos,
    })))
}

#[derive(Deserialize)]
struct RegistrarAjuste {
    id: Option<Uuid>,
    /// Con signo: positivo aumenta la deuda, negativo la reduce.
    monto_centavos: i64,
    motivo: String,
}

/// Ajuste manual de cuenta (condonación, corrección de un error de carga).
/// Correcciones = contra-asientos, nunca UPDATE/DELETE sobre el ledger.
async fn registrar_ajuste(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(cliente_id): Path<Uuid>,
    Json(datos): Json<RegistrarAjuste>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CLIENTES)?;
    if datos.monto_centavos == 0 {
        return Err(ErrorApi::Validacion("el monto no puede ser cero".into()));
    }
    if datos.motivo.trim().is_empty() {
        return Err(ErrorApi::Validacion("el motivo es obligatorio".into()));
    }

    let movimiento_id = datos.id.unwrap_or_else(Uuid::now_v7);
    let mut tx = estado.pool.begin().await?;

    sqlx::query!(
        r#"SELECT id FROM clientes.clientes WHERE id = $1 FOR UPDATE"#,
        cliente_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let insertado = sqlx::query!(
        r#"
        INSERT INTO clientes.cuenta_movimientos
            (id, cliente_id, tipo, monto_centavos, motivo, usuario_id)
        VALUES ($1, $2, 'ajuste', $3, $4, $5)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        movimiento_id,
        cliente_id,
        datos.monto_centavos,
        datos.motivo.trim(),
        usuario.id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    if insertado.is_none() {
        return Ok(Json(json!({ "id": movimiento_id, "ya_estaba_aplicado": true })));
    }

    actualizar_saldo(&mut tx, cliente_id, datos.monto_centavos).await?;
    // Un ajuste negativo condona deuda: consume renglones pendientes por
    // FIFO igual que un pago, para que dejen de revalorizarse.
    if datos.monto_centavos < 0 {
        crate::clientes::aplicar_condonacion_fifo(
            &mut tx,
            cliente_id,
            movimiento_id,
            -datos.monto_centavos,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "id": movimiento_id, "ya_estaba_aplicado": false })))
}
