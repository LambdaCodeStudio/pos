use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;
use crate::inventario;
use crate::ventas::{EstadoVenta, MedioPago};

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/sesiones", get(listar_sesiones).post(abrir_sesion))
        .route("/sesiones/{id}", get(obtener_sesion))
        .route("/sesiones/{id}/cerrar", post(cerrar_sesion))
        .route("/", get(listar_ventas).post(sincronizar_venta))
        .route("/{id}", get(obtener_venta))
        .route("/{id}/anular", post(anular_venta))
}

// ---------- Sesiones de caja ----------

#[derive(Deserialize)]
struct AbrirSesion {
    /// UUID generado por el dispositivo (idempotencia).
    id: Option<Uuid>,
    monto_inicial_centavos: i64,
}

async fn abrir_sesion(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<AbrirSesion>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::ABRIR_CAJA)?;
    if datos.monto_inicial_centavos < 0 {
        return Err(ErrorApi::Validacion("el monto inicial no puede ser negativo".into()));
    }

    let id = datos.id.unwrap_or_else(Uuid::now_v7);
    let mut tx = estado.pool.begin().await?;

    // Idempotencia por UUID del dispositivo.
    let existente = sqlx::query!(
        r#"SELECT id, abierta_en, cerrada_en FROM ventas.sesiones_caja WHERE id = $1"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(s) = existente {
        return Ok(Json(json!({
            "id": s.id,
            "abierta_en": s.abierta_en,
            "ya_existia": true,
        })));
    }

    let abierta = sqlx::query!(
        r#"
        SELECT id FROM ventas.sesiones_caja
        WHERE usuario_id = $1 AND cerrada_en IS NULL
        "#,
        usuario.id,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if abierta.is_some() {
        return Err(ErrorApi::Conflicto(
            "ya tenés una sesión de caja abierta; cerrala antes de abrir otra".into(),
        ));
    }

    let fila = sqlx::query!(
        r#"
        INSERT INTO ventas.sesiones_caja (id, usuario_id, monto_inicial_centavos)
        VALUES ($1, $2, $3)
        RETURNING abierta_en
        "#,
        id,
        usuario.id,
        datos.monto_inicial_centavos,
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "id": id, "abierta_en": fila.abierta_en, "ya_existia": false })))
}

#[derive(Deserialize)]
struct CerrarSesion {
    monto_contado_centavos: i64,
}

/// Cierra la sesión registrando el arqueo. El efectivo esperado se calcula
/// del ledger de pagos (monto inicial + pagos en efectivo de ventas
/// confirmadas). La diferencia queda registrada tal cual: NUNCA se corrige.
async fn cerrar_sesion(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<CerrarSesion>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::CERRAR_CAJA)?;
    if datos.monto_contado_centavos < 0 {
        return Err(ErrorApi::Validacion("el monto contado no puede ser negativo".into()));
    }

    let mut tx = estado.pool.begin().await?;

    let sesion = sqlx::query!(
        r#"
        SELECT monto_inicial_centavos, cerrada_en, monto_contado_centavos,
               diferencia_arqueo_centavos
        FROM ventas.sesiones_caja WHERE id = $1 FOR UPDATE
        "#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    // Idempotente: si ya está cerrada devuelve el arqueo registrado.
    if sesion.cerrada_en.is_some() {
        return Ok(Json(json!({
            "id": id,
            "ya_estaba_cerrada": true,
            "monto_contado_centavos": sesion.monto_contado_centavos,
            "diferencia_arqueo_centavos": sesion.diferencia_arqueo_centavos,
        })));
    }

    let efectivo_ventas = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(p.monto_centavos), 0)::bigint AS "total!"
        FROM ventas.pagos p
        JOIN ventas.ventas v ON v.id = p.venta_id
        WHERE v.sesion_id = $1 AND v.estado = 'confirmada' AND p.medio = 'efectivo'
        "#,
        id,
    )
    .fetch_one(&mut *tx)
    .await?;

    let esperado = sesion.monto_inicial_centavos + efectivo_ventas;
    let diferencia = datos.monto_contado_centavos - esperado;

    sqlx::query!(
        r#"
        UPDATE ventas.sesiones_caja
        SET cerrada_en = now(), monto_contado_centavos = $2, diferencia_arqueo_centavos = $3
        WHERE id = $1
        "#,
        id,
        datos.monto_contado_centavos,
        diferencia,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "id": id,
        "efectivo_esperado_centavos": esperado,
        "monto_contado_centavos": datos.monto_contado_centavos,
        "diferencia_arqueo_centavos": diferencia,
    })))
}

#[derive(Serialize)]
struct SesionResumen {
    id: Uuid,
    usuario_id: Uuid,
    usuario_nombre: String,
    monto_inicial_centavos: i64,
    abierta_en: DateTime<Utc>,
    cerrada_en: Option<DateTime<Utc>>,
    monto_contado_centavos: Option<i64>,
    diferencia_arqueo_centavos: Option<i64>,
    cantidad_ventas: i64,
}

#[derive(Deserialize)]
struct FiltroSesiones {
    solo_abiertas: Option<bool>,
    limite: Option<i64>,
}

async fn listar_sesiones(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroSesiones>,
) -> Result<Json<Vec<SesionResumen>>, ErrorApi> {
    let solo_abiertas = filtro.solo_abiertas.unwrap_or(false);
    let limite = filtro.limite.unwrap_or(50).clamp(1, 200);
    let filas = sqlx::query!(
        r#"
        SELECT s.id, s.usuario_id, u.nombre AS usuario_nombre, s.monto_inicial_centavos,
               s.abierta_en, s.cerrada_en, s.monto_contado_centavos,
               s.diferencia_arqueo_centavos,
               COUNT(v.id) AS "cantidad_ventas!"
        FROM ventas.sesiones_caja s
        JOIN identidad.usuarios u ON u.id = s.usuario_id
        LEFT JOIN ventas.ventas v ON v.sesion_id = s.id
        WHERE s.cerrada_en IS NULL OR NOT $1
        GROUP BY s.id, s.usuario_id, u.nombre, s.monto_inicial_centavos, s.abierta_en,
                 s.cerrada_en, s.monto_contado_centavos, s.diferencia_arqueo_centavos
        ORDER BY s.abierta_en DESC
        LIMIT $2
        "#,
        solo_abiertas,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| SesionResumen {
                id: f.id,
                usuario_id: f.usuario_id,
                usuario_nombre: f.usuario_nombre,
                monto_inicial_centavos: f.monto_inicial_centavos,
                abierta_en: f.abierta_en,
                cerrada_en: f.cerrada_en,
                monto_contado_centavos: f.monto_contado_centavos,
                diferencia_arqueo_centavos: f.diferencia_arqueo_centavos,
                cantidad_ventas: f.cantidad_ventas,
            })
            .collect(),
    ))
}

async fn obtener_sesion(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let s = sqlx::query!(
        r#"
        SELECT s.id, s.usuario_id, u.nombre AS usuario_nombre, s.monto_inicial_centavos,
               s.abierta_en, s.cerrada_en, s.monto_contado_centavos, s.diferencia_arqueo_centavos
        FROM ventas.sesiones_caja s
        JOIN identidad.usuarios u ON u.id = s.usuario_id
        WHERE s.id = $1
        "#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    // Totales por medio de pago (solo ventas confirmadas): la pantalla de
    // arqueo de la PWA los muestra antes de cerrar.
    let totales = sqlx::query!(
        r#"
        SELECT p.medio AS "medio: MedioPago", SUM(p.monto_centavos)::bigint AS "total!"
        FROM ventas.pagos p
        JOIN ventas.ventas v ON v.id = p.venta_id
        WHERE v.sesion_id = $1 AND v.estado = 'confirmada'
        GROUP BY p.medio
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "id": s.id,
        "usuario_id": s.usuario_id,
        "usuario_nombre": s.usuario_nombre,
        "monto_inicial_centavos": s.monto_inicial_centavos,
        "abierta_en": s.abierta_en,
        "cerrada_en": s.cerrada_en,
        "monto_contado_centavos": s.monto_contado_centavos,
        "diferencia_arqueo_centavos": s.diferencia_arqueo_centavos,
        "totales_por_medio": totales.into_iter().map(|t| json!({
            "medio": t.medio,
            "total_centavos": t.total,
        })).collect::<Vec<_>>(),
    })))
}

// ---------- Sincronización de ventas ----------

#[derive(Deserialize)]
struct ItemVenta {
    producto_id: Uuid,
    /// Snapshots del dispositivo; si faltan se completan del catálogo.
    producto_nombre: Option<String>,
    precio_unitario_centavos: i64,
    cantidad: Decimal,
    iva_pct: Option<Decimal>,
    #[serde(default)]
    descuento_centavos: i64,
    descuento_motivo: Option<String>,
    subtotal_centavos: i64,
}

#[derive(Deserialize)]
struct PagoVenta {
    medio: MedioPago,
    monto_centavos: i64,
    referencia_externa: Option<String>,
}

#[derive(Deserialize)]
struct SincronizarVenta {
    /// UUID generado en el dispositivo: la llave de la idempotencia.
    id: Uuid,
    sesion_id: Uuid,
    cliente_id: Option<Uuid>,
    total_centavos: i64,
    #[serde(default)]
    descuento_centavos: i64,
    descuento_motivo: Option<String>,
    /// Reloj del dispositivo al momento de la venta.
    vendida_en: DateTime<Utc>,
    items: Vec<ItemVenta>,
    pagos: Vec<PagoVenta>,
}

/// Sincroniza una venta que llega YA CONFIRMADA desde el dispositivo.
/// Idempotente por UUID (reintento = no-op). El servidor es el único
/// escritor del ledger: genera los movimientos de stock con FEFO por
/// asunción. La caja no sabe de lotes.
async fn sincronizar_venta(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<SincronizarVenta>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VENDER)?;

    if datos.items.is_empty() {
        return Err(ErrorApi::Validacion("la venta no tiene ítems".into()));
    }
    if datos.pagos.is_empty() {
        return Err(ErrorApi::Validacion("la venta no tiene pagos".into()));
    }
    if datos.total_centavos < 0 || datos.descuento_centavos < 0 {
        return Err(ErrorApi::Validacion("montos negativos".into()));
    }

    // Invariante: Σ pagos = total.
    let suma_pagos: i64 = datos.pagos.iter().map(|p| p.monto_centavos).sum();
    if suma_pagos != datos.total_centavos {
        return Err(ErrorApi::Validacion(format!(
            "la suma de pagos ({suma_pagos}) no coincide con el total ({})",
            datos.total_centavos
        )));
    }

    // Coherencia del documento: total = Σ subtotales − descuento de ticket.
    let suma_subtotales: i64 = datos.items.iter().map(|i| i.subtotal_centavos).sum();
    if suma_subtotales - datos.descuento_centavos != datos.total_centavos {
        return Err(ErrorApi::Validacion(format!(
            "el total ({}) no coincide con Σ subtotales ({suma_subtotales}) − descuento ({})",
            datos.total_centavos, datos.descuento_centavos
        )));
    }

    // El pago con cuenta corriente exige un cliente identificado.
    let monto_cuenta_corriente: i64 = datos
        .pagos
        .iter()
        .filter(|p| p.medio == MedioPago::CuentaCorriente)
        .map(|p| p.monto_centavos)
        .sum();
    if monto_cuenta_corriente > 0 && datos.cliente_id.is_none() {
        return Err(ErrorApi::Validacion(
            "el pago con cuenta corriente requiere un cliente identificado".into(),
        ));
    }
    // El fiado es todo-o-nada por venta: no se mezcla con otros medios de
    // pago. Así cada ítem de la venta es, sin ambigüedad, un renglón
    // pendiente en la cuenta del cliente (ver clientes::registrar_cargo_de_venta).
    if monto_cuenta_corriente > 0 && monto_cuenta_corriente != datos.total_centavos {
        return Err(ErrorApi::Validacion(
            "el fiado no se puede combinar con otro medio de pago en el mismo ticket".into(),
        ));
    }

    for item in &datos.items {
        if item.cantidad <= Decimal::ZERO {
            return Err(ErrorApi::Validacion("cantidades deben ser positivas".into()));
        }
        if item.precio_unitario_centavos < 0
            || item.descuento_centavos < 0
            || item.subtotal_centavos < 0
        {
            return Err(ErrorApi::Validacion("montos de ítem negativos".into()));
        }
    }

    let mut tx = estado.pool.begin().await?;

    sqlx::query!(r#"SELECT id FROM ventas.sesiones_caja WHERE id = $1"#, datos.sesion_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ErrorApi::Validacion("sesión de caja inexistente".into()))?;

    // Idempotencia: el INSERT de la cabecera es la barrera. Si el UUID ya
    // existe, la venta ya fue procesada (movimientos incluidos): no-op.
    let insertada = sqlx::query!(
        r#"
        INSERT INTO ventas.ventas
            (id, sesion_id, cliente_id, total_centavos, descuento_centavos,
             descuento_motivo, usuario_id, vendida_en)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        datos.id,
        datos.sesion_id,
        datos.cliente_id,
        datos.total_centavos,
        datos.descuento_centavos,
        datos.descuento_motivo,
        usuario.id,
        datos.vendida_en,
    )
    .fetch_optional(&mut *tx)
    .await?;

    if insertada.is_none() {
        return Ok(Json(json!({ "id": datos.id, "ya_estaba_sincronizada": true })));
    }

    let mut items_fiado = Vec::new();
    for item in &datos.items {
        // Snapshots: lo que mandó el dispositivo manda; el catálogo completa.
        let producto = sqlx::query!(
            r#"
            SELECT p.nombre, COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct!"
            FROM catalogo.productos p
            JOIN catalogo.categorias c ON c.id = p.categoria_id
            WHERE p.id = $1
            "#,
            item.producto_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ErrorApi::Validacion("producto inexistente en la venta".into()))?;

        let producto_nombre = item.producto_nombre.as_deref().unwrap_or(&producto.nombre);

        let item_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO ventas.venta_items
                (id, venta_id, producto_id, producto_nombre, precio_unitario_centavos,
                 cantidad, iva_pct, descuento_centavos, descuento_motivo, subtotal_centavos)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            item_id,
            datos.id,
            item.producto_id,
            producto_nombre,
            item.precio_unitario_centavos,
            item.cantidad,
            item.iva_pct.unwrap_or(producto.iva_pct),
            item.descuento_centavos,
            item.descuento_motivo,
            item.subtotal_centavos,
        )
        .execute(&mut *tx)
        .await?;

        inventario::registrar_salida_venta_fefo(
            &mut tx,
            item.producto_id,
            item.cantidad,
            item_id,
            usuario.id,
        )
        .await?;

        if monto_cuenta_corriente > 0 {
            items_fiado.push(crate::clientes::ItemFiado {
                producto_id: item.producto_id,
                producto_nombre: producto_nombre.to_string(),
                cantidad: item.cantidad,
            });
        }
    }

    for pago in &datos.pagos {
        sqlx::query!(
            r#"
            INSERT INTO ventas.pagos (id, venta_id, medio, monto_centavos, referencia_externa)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            Uuid::now_v7(),
            datos.id,
            pago.medio as MedioPago,
            pago.monto_centavos,
            pago.referencia_externa,
        )
        .execute(&mut *tx)
        .await?;
    }

    // El fiado inserta su cargo en el ledger de Clientes en ESTA transacción,
    // referenciando la venta. El límite de crédito bloquea acá.
    if monto_cuenta_corriente > 0 {
        crate::clientes::registrar_cargo_de_venta(
            &mut tx,
            datos.cliente_id.unwrap(),
            datos.id,
            monto_cuenta_corriente,
            &items_fiado,
            &usuario,
        )
        .await?;
    }

    tx.commit().await?;

    Ok(Json(json!({
        "id": datos.id,
        "ya_estaba_sincronizada": false,
        "items": datos.items.len(),
    })))
}

// ---------- Anulación ----------

#[derive(Deserialize, Default)]
struct AnularVenta {
    motivo: Option<String>,
}

/// Anula una venta: estado `anulada` + contra-asientos en el ledger de stock
/// referenciando los ítems originales. Jamás se edita una venta. Idempotente:
/// anular una venta ya anulada es un no-op.
async fn anular_venta(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    cuerpo: Option<Json<AnularVenta>>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::ANULAR_VENTA)?;
    let motivo = cuerpo.and_then(|Json(c)| c.motivo);

    let mut tx = estado.pool.begin().await?;

    let venta = sqlx::query!(
        r#"SELECT estado AS "estado: EstadoVenta" FROM ventas.ventas WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    if venta.estado == EstadoVenta::Anulada {
        return Ok(Json(json!({ "id": id, "ya_estaba_anulada": true })));
    }

    let items = sqlx::query!(r#"SELECT id FROM ventas.venta_items WHERE venta_id = $1"#, id)
        .fetch_all(&mut *tx)
        .await?;

    for item in &items {
        inventario::registrar_reversa_venta(&mut tx, item.id, usuario.id).await?;
    }

    // Si la venta tenía cargo en cuenta corriente, contra-asiento del fiado.
    crate::clientes::revertir_cargos_de_venta(&mut tx, id, usuario.id).await?;

    sqlx::query!(
        r#"
        UPDATE ventas.ventas
        SET estado = 'anulada', anulada_en = now(), anulada_por = $2, anulacion_motivo = $3
        WHERE id = $1
        "#,
        id,
        usuario.id,
        motivo,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "id": id, "ya_estaba_anulada": false, "items_revertidos": items.len() })))
}

// ---------- Consultas ----------

#[derive(Serialize)]
struct VentaResumen {
    id: Uuid,
    sesion_id: Uuid,
    cliente_id: Option<Uuid>,
    total_centavos: i64,
    descuento_centavos: i64,
    estado: EstadoVenta,
    usuario_id: Uuid,
    vendida_en: DateTime<Utc>,
    sincronizada_en: DateTime<Utc>,
}

#[derive(Deserialize)]
struct FiltroVentas {
    sesion_id: Option<Uuid>,
    limite: Option<i64>,
}

async fn listar_ventas(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroVentas>,
) -> Result<Json<Vec<VentaResumen>>, ErrorApi> {
    let limite = filtro.limite.unwrap_or(50).clamp(1, 200);
    let filas = sqlx::query_as!(
        VentaResumen,
        r#"
        SELECT id, sesion_id, cliente_id, total_centavos, descuento_centavos,
               estado AS "estado: EstadoVenta", usuario_id, vendida_en, sincronizada_en
        FROM ventas.ventas
        WHERE ($1::uuid IS NULL OR sesion_id = $1)
        ORDER BY vendida_en DESC
        LIMIT $2
        "#,
        filtro.sesion_id,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;
    Ok(Json(filas))
}

async fn obtener_venta(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let v = sqlx::query!(
        r#"
        SELECT id, sesion_id, cliente_id, total_centavos, descuento_centavos,
               descuento_motivo, estado AS "estado: EstadoVenta", usuario_id,
               vendida_en, sincronizada_en, anulada_en, anulada_por, anulacion_motivo
        FROM ventas.ventas WHERE id = $1
        "#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let items = sqlx::query!(
        r#"
        SELECT id, producto_id, producto_nombre, precio_unitario_centavos, cantidad,
               iva_pct, descuento_centavos, descuento_motivo, subtotal_centavos
        FROM ventas.venta_items WHERE venta_id = $1
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    let pagos = sqlx::query!(
        r#"
        SELECT id, medio AS "medio: MedioPago", monto_centavos, referencia_externa
        FROM ventas.pagos WHERE venta_id = $1
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "id": v.id,
        "sesion_id": v.sesion_id,
        "cliente_id": v.cliente_id,
        "total_centavos": v.total_centavos,
        "descuento_centavos": v.descuento_centavos,
        "descuento_motivo": v.descuento_motivo,
        "estado": v.estado,
        "usuario_id": v.usuario_id,
        "vendida_en": v.vendida_en,
        "sincronizada_en": v.sincronizada_en,
        "anulada_en": v.anulada_en,
        "anulada_por": v.anulada_por,
        "anulacion_motivo": v.anulacion_motivo,
        "items": items.into_iter().map(|i| json!({
            "id": i.id,
            "producto_id": i.producto_id,
            "producto_nombre": i.producto_nombre,
            "precio_unitario_centavos": i.precio_unitario_centavos,
            "cantidad": i.cantidad,
            "iva_pct": i.iva_pct,
            "descuento_centavos": i.descuento_centavos,
            "descuento_motivo": i.descuento_motivo,
            "subtotal_centavos": i.subtotal_centavos,
        })).collect::<Vec<_>>(),
        "pagos": pagos.into_iter().map(|p| json!({
            "id": p.id,
            "medio": p.medio,
            "monto_centavos": p.monto_centavos,
            "referencia_externa": p.referencia_externa,
        })).collect::<Vec<_>>(),
    })))
}
