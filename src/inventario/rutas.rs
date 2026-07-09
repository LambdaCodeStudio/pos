use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;
use crate::inventario::{actualizar_stock_actual, MotivoAjuste, TipoMovimiento, DEPOSITO_PRINCIPAL_ID};

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/ajustes", get(listar_ajustes).post(crear_ajuste))
        .route("/ajustes/{id}", get(obtener_ajuste))
        .route("/productos/{id}/stock", get(stock_de_producto))
        .route("/alertas-vencimiento", get(alertas_vencimiento))
        .route("/movimientos", get(listar_movimientos))
}

// ---------- Ajustes ----------

#[derive(Deserialize)]
struct ItemAjuste {
    producto_id: Uuid,
    lote_id: Option<Uuid>,
    /// Delta con signo (entradas +, salidas −). Excluyente con cantidad_contada.
    delta: Option<Decimal>,
    /// Cantidad física contada: el delta se calcula contra la proyección
    /// (del lote si se indicó, si no del stock del depósito). Para conteos.
    cantidad_contada: Option<Decimal>,
}

#[derive(Deserialize)]
struct CrearAjuste {
    /// UUID generado por el cliente para idempotencia.
    id: Option<Uuid>,
    motivo: MotivoAjuste,
    observaciones: Option<String>,
    items: Vec<ItemAjuste>,
}

#[derive(Serialize)]
struct MovimientoAplicado {
    producto_id: Uuid,
    lote_id: Option<Uuid>,
    delta: Decimal,
    stock_resultante: Decimal,
}

/// Crea un documento de ajuste y sus movimientos en el ledger, actualizando
/// proyecciones (stock y lotes) en la misma transacción. Idempotente por UUID:
/// si el ajuste ya existe, no se reaplica nada.
///
/// A diferencia de las ventas (que pueden dejar stock negativo), los ajustes
/// SÍ validan disponibilidad: un ajuste negativo no puede dejar el stock ni el
/// lote por debajo de cero.
async fn crear_ajuste(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearAjuste>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::AJUSTAR_STOCK)?;
    if datos.items.is_empty() {
        return Err(ErrorApi::Validacion("el ajuste no tiene ítems".into()));
    }

    let ajuste_id = datos.id.unwrap_or_else(Uuid::now_v7);
    let mut tx = estado.pool.begin().await?;

    let insertado = sqlx::query!(
        r#"
        INSERT INTO inventario.ajustes (id, motivo, observaciones, usuario_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        ajuste_id,
        datos.motivo as MotivoAjuste,
        datos.observaciones,
        usuario.id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    if insertado.is_none() {
        // Reintento de un ajuste ya procesado: no-op exitoso.
        return Ok(Json(json!({ "id": ajuste_id, "ya_estaba_aplicado": true })));
    }

    let mut aplicados: Vec<MovimientoAplicado> = Vec::new();

    for item in &datos.items {
        let delta = match (item.delta, item.cantidad_contada) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(ErrorApi::Validacion(
                    "cada ítem lleva delta o cantidad_contada, exactamente uno".into(),
                ))
            }
            (Some(d), None) => {
                if d == Decimal::ZERO {
                    return Err(ErrorApi::Validacion("el delta no puede ser cero".into()));
                }
                d
            }
            (None, Some(contada)) => {
                if contada < Decimal::ZERO {
                    return Err(ErrorApi::Validacion(
                        "la cantidad contada no puede ser negativa".into(),
                    ));
                }
                let actual = cantidad_actual(&mut tx, item.producto_id, item.lote_id).await?;
                contada - actual
            }
        };

        // Un conteo que coincide con la proyección no genera movimiento
        // (el ledger prohíbe cantidad = 0).
        if delta == Decimal::ZERO {
            continue;
        }

        sqlx::query!(
            r#"SELECT id FROM catalogo.productos WHERE id = $1"#,
            item.producto_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ErrorApi::Validacion("producto inexistente".into()))?;

        // Validación de disponibilidad sobre el lote (si aplica).
        if let Some(lote_id) = item.lote_id {
            let lote = sqlx::query!(
                r#"SELECT producto_id, cantidad_actual FROM inventario.lotes
                   WHERE id = $1 FOR UPDATE"#,
                lote_id,
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("lote inexistente".into()))?;

            if lote.producto_id != item.producto_id {
                return Err(ErrorApi::Validacion(
                    "el lote no pertenece al producto indicado".into(),
                ));
            }
            if lote.cantidad_actual + delta < Decimal::ZERO {
                return Err(ErrorApi::Validacion(format!(
                    "el ajuste dejaría el lote en negativo (actual {}, delta {})",
                    lote.cantidad_actual, delta
                )));
            }

            sqlx::query!(
                r#"UPDATE inventario.lotes SET cantidad_actual = cantidad_actual + $2 WHERE id = $1"#,
                lote_id,
                delta,
            )
            .execute(&mut *tx)
            .await?;
        }

        // Validación de disponibilidad sobre el stock del depósito.
        let stock = sqlx::query!(
            r#"SELECT cantidad FROM inventario.stock_actual
               WHERE producto_id = $1 AND deposito_id = $2 FOR UPDATE"#,
            item.producto_id,
            DEPOSITO_PRINCIPAL_ID,
        )
        .fetch_optional(&mut *tx)
        .await?
        .map(|f| f.cantidad)
        .unwrap_or(Decimal::ZERO);

        if stock + delta < Decimal::ZERO {
            return Err(ErrorApi::Validacion(format!(
                "el ajuste dejaría el stock en negativo (actual {stock}, delta {delta})"
            )));
        }

        sqlx::query!(
            r#"
            INSERT INTO inventario.movimientos_stock
                (id, producto_id, deposito_id, lote_id, cantidad, tipo, ajuste_id, usuario_id)
            VALUES ($1, $2, $3, $4, $5, 'ajuste', $6, $7)
            "#,
            Uuid::now_v7(),
            item.producto_id,
            DEPOSITO_PRINCIPAL_ID,
            item.lote_id,
            delta,
            ajuste_id,
            usuario.id,
        )
        .execute(&mut *tx)
        .await?;

        actualizar_stock_actual(&mut tx, item.producto_id, DEPOSITO_PRINCIPAL_ID, delta).await?;

        aplicados.push(MovimientoAplicado {
            producto_id: item.producto_id,
            lote_id: item.lote_id,
            delta,
            stock_resultante: stock + delta,
        });
    }

    tx.commit().await?;

    Ok(Json(json!({
        "id": ajuste_id,
        "motivo": datos.motivo,
        "movimientos": aplicados,
    })))
}

/// Proyección vigente contra la que se compara un conteo: la del lote si se
/// indicó, si no la del stock del depósito principal.
async fn cantidad_actual(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    producto_id: Uuid,
    lote_id: Option<Uuid>,
) -> Result<Decimal, ErrorApi> {
    match lote_id {
        Some(lote_id) => {
            let lote = sqlx::query!(
                r#"SELECT producto_id, cantidad_actual FROM inventario.lotes WHERE id = $1"#,
                lote_id,
            )
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("lote inexistente".into()))?;
            if lote.producto_id != producto_id {
                return Err(ErrorApi::Validacion(
                    "el lote no pertenece al producto indicado".into(),
                ));
            }
            Ok(lote.cantidad_actual)
        }
        None => Ok(sqlx::query!(
            r#"SELECT cantidad FROM inventario.stock_actual
               WHERE producto_id = $1 AND deposito_id = $2"#,
            producto_id,
            DEPOSITO_PRINCIPAL_ID,
        )
        .fetch_optional(&mut **tx)
        .await?
        .map(|f| f.cantidad)
        .unwrap_or(Decimal::ZERO)),
    }
}

#[derive(Serialize)]
struct AjusteResumen {
    id: Uuid,
    motivo: MotivoAjuste,
    observaciones: Option<String>,
    usuario_id: Uuid,
    creado_en: DateTime<Utc>,
    cantidad_movimientos: i64,
}

#[derive(Deserialize)]
struct FiltroAjustes {
    limite: Option<i64>,
}

async fn listar_ajustes(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroAjustes>,
) -> Result<Json<Vec<AjusteResumen>>, ErrorApi> {
    let limite = filtro.limite.unwrap_or(50).clamp(1, 200);
    let filas = sqlx::query!(
        r#"
        SELECT a.id, a.motivo AS "motivo: MotivoAjuste", a.observaciones, a.usuario_id,
               a.creado_en, COUNT(m.id) AS "cantidad_movimientos!"
        FROM inventario.ajustes a
        LEFT JOIN inventario.movimientos_stock m ON m.ajuste_id = a.id
        GROUP BY a.id, a.motivo, a.observaciones, a.usuario_id, a.creado_en
        ORDER BY a.creado_en DESC
        LIMIT $1
        "#,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| AjusteResumen {
                id: f.id,
                motivo: f.motivo,
                observaciones: f.observaciones,
                usuario_id: f.usuario_id,
                creado_en: f.creado_en,
                cantidad_movimientos: f.cantidad_movimientos,
            })
            .collect(),
    ))
}

async fn obtener_ajuste(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let cab = sqlx::query!(
        r#"SELECT id, motivo AS "motivo: MotivoAjuste", observaciones, usuario_id, creado_en
           FROM inventario.ajustes WHERE id = $1"#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let movimientos = sqlx::query!(
        r#"
        SELECT m.id, m.producto_id, p.nombre AS producto_nombre, m.lote_id, m.cantidad, m.creado_en
        FROM inventario.movimientos_stock m
        JOIN catalogo.productos p ON p.id = m.producto_id
        WHERE m.ajuste_id = $1
        ORDER BY m.creado_en
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "id": cab.id,
        "motivo": cab.motivo,
        "observaciones": cab.observaciones,
        "usuario_id": cab.usuario_id,
        "creado_en": cab.creado_en,
        "movimientos": movimientos.into_iter().map(|m| json!({
            "id": m.id,
            "producto_id": m.producto_id,
            "producto_nombre": m.producto_nombre,
            "lote_id": m.lote_id,
            "cantidad": m.cantidad,
            "creado_en": m.creado_en,
        })).collect::<Vec<_>>(),
    })))
}

// ---------- Stock en pantalla de producto ----------

#[derive(Serialize)]
struct LoteDeProducto {
    id: Uuid,
    codigo_lote: Option<String>,
    vencimiento: NaiveDate,
    cantidad_actual: Decimal,
}

async fn stock_de_producto(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    sqlx::query!(r#"SELECT id FROM catalogo.productos WHERE id = $1"#, id)
        .fetch_optional(&estado.pool)
        .await?
        .ok_or(ErrorApi::NoEncontrado)?;

    let cantidad = sqlx::query!(
        r#"SELECT cantidad FROM inventario.stock_actual
           WHERE producto_id = $1 AND deposito_id = $2"#,
        id,
        DEPOSITO_PRINCIPAL_ID,
    )
    .fetch_optional(&estado.pool)
    .await?
    .map(|f| f.cantidad)
    .unwrap_or(Decimal::ZERO);

    let lotes = sqlx::query_as!(
        LoteDeProducto,
        r#"
        SELECT id, codigo_lote, vencimiento, cantidad_actual
        FROM inventario.lotes
        WHERE producto_id = $1 AND cantidad_actual > 0
        ORDER BY vencimiento
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "producto_id": id,
        "deposito_id": DEPOSITO_PRINCIPAL_ID,
        "cantidad": cantidad,
        "lotes": lotes,
    })))
}

// ---------- Alertas de vencimiento ----------

#[derive(Serialize)]
struct AlertaVencimiento {
    lote_id: Uuid,
    producto_id: Uuid,
    producto_nombre: String,
    codigo_lote: Option<String>,
    vencimiento: NaiveDate,
    dias_restantes: i32,
    cantidad_actual: Decimal,
}

#[derive(Deserialize)]
struct FiltroAlertas {
    /// Ventana en días hacia adelante (default 30). Incluye lo ya vencido.
    dias: Option<i32>,
}

/// Lotes con stock cuyo vencimiento cae dentro de la ventana pedida —
/// accionables: el encargado decide rebajar, tirar (ajuste por vencimiento)
/// o devolver al proveedor.
async fn alertas_vencimiento(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroAlertas>,
) -> Result<Json<Vec<AlertaVencimiento>>, ErrorApi> {
    let dias = filtro.dias.unwrap_or(30).clamp(0, 365);
    let filas = sqlx::query!(
        r#"
        SELECT l.id AS lote_id, l.producto_id, p.nombre AS producto_nombre,
               l.codigo_lote, l.vencimiento,
               (l.vencimiento - CURRENT_DATE) AS "dias_restantes!",
               l.cantidad_actual
        FROM inventario.lotes l
        JOIN catalogo.productos p ON p.id = l.producto_id
        WHERE l.cantidad_actual > 0
          AND l.vencimiento <= CURRENT_DATE + $1::int
        ORDER BY l.vencimiento, p.nombre
        "#,
        dias,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| AlertaVencimiento {
                lote_id: f.lote_id,
                producto_id: f.producto_id,
                producto_nombre: f.producto_nombre,
                codigo_lote: f.codigo_lote,
                vencimiento: f.vencimiento,
                dias_restantes: f.dias_restantes,
                cantidad_actual: f.cantidad_actual,
            })
            .collect(),
    ))
}

// ---------- Consulta del ledger ----------

#[derive(Serialize)]
struct MovimientoStock {
    id: Uuid,
    producto_id: Uuid,
    producto_nombre: String,
    deposito_id: Uuid,
    lote_id: Option<Uuid>,
    cantidad: Decimal,
    tipo: TipoMovimiento,
    recepcion_item_id: Option<Uuid>,
    venta_item_id: Option<Uuid>,
    ajuste_id: Option<Uuid>,
    usuario_id: Uuid,
    creado_en: DateTime<Utc>,
}

#[derive(Deserialize)]
struct FiltroMovimientos {
    producto_id: Option<Uuid>,
    limite: Option<i64>,
}

async fn listar_movimientos(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(filtro): Query<FiltroMovimientos>,
) -> Result<Json<Vec<MovimientoStock>>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let limite = filtro.limite.unwrap_or(100).clamp(1, 500);
    let filas = sqlx::query!(
        r#"
        SELECT m.id, m.producto_id, p.nombre AS producto_nombre, m.deposito_id,
               m.lote_id, m.cantidad, m.tipo AS "tipo: TipoMovimiento",
               m.recepcion_item_id, m.venta_item_id, m.ajuste_id,
               m.usuario_id, m.creado_en
        FROM inventario.movimientos_stock m
        JOIN catalogo.productos p ON p.id = m.producto_id
        WHERE ($1::uuid IS NULL OR m.producto_id = $1)
        ORDER BY m.creado_en DESC
        LIMIT $2
        "#,
        filtro.producto_id,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| MovimientoStock {
                id: f.id,
                producto_id: f.producto_id,
                producto_nombre: f.producto_nombre,
                deposito_id: f.deposito_id,
                lote_id: f.lote_id,
                cantidad: f.cantidad,
                tipo: f.tipo,
                recepcion_item_id: f.recepcion_item_id,
                venta_item_id: f.venta_item_id,
                ajuste_id: f.ajuste_id,
                usuario_id: f.usuario_id,
                creado_en: f.creado_en,
            })
            .collect(),
    ))
}
