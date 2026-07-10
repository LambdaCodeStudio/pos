use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;
use crate::inventario::MotivoAjuste;
use crate::reportes::ZONA_HORARIA;
use crate::ventas::MedioPago;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/ventas-resumen", get(ventas_resumen))
        .route("/top-productos", get(top_productos))
        .route("/productos-sin-movimiento", get(productos_sin_movimiento))
        .route("/ventas-por-vendedor", get(ventas_por_vendedor))
        .route("/mermas", get(mermas))
        .route("/fiado", get(fiado))
        .route("/inventario", get(inventario))
        .route("/arqueos", get(arqueos))
        .route("/compras-resumen", get(compras_resumen))
}

#[derive(Deserialize)]
struct RangoFechas {
    desde: Option<NaiveDate>,
    hasta: Option<NaiveDate>,
    limite: Option<i64>,
}

impl RangoFechas {
    /// Default: últimos 30 días, ambos extremos inclusive.
    fn resolver(&self) -> (NaiveDate, NaiveDate) {
        let hoy = chrono::Utc::now()
            .with_timezone(&chrono_tz(ZONA_HORARIA))
            .date_naive();
        let hasta = self.hasta.unwrap_or(hoy);
        let desde = self.desde.unwrap_or(hasta - chrono::Duration::days(29));
        (desde, hasta)
    }
}

/// chrono-tz sin la dependencia: para el resumen alcanza con el offset fijo
/// de Argentina (-03:00, sin horario de verano desde 2009).
fn chrono_tz(_zona: &str) -> chrono::FixedOffset {
    chrono::FixedOffset::west_opt(3 * 3600).expect("offset válido")
}

async fn ventas_resumen(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();

    let totales = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(total_centavos), 0)::bigint AS "facturado!",
               COUNT(*) AS "tickets!",
               COALESCE(SUM(descuento_centavos), 0)::bigint AS "descuentos!"
        FROM ventas.ventas
        WHERE estado = 'confirmada'
          AND (vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_one(&estado.pool)
    .await?;

    let anuladas = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "cantidad!", COALESCE(SUM(total_centavos), 0)::bigint AS "total_centavos!"
        FROM ventas.ventas
        WHERE estado = 'anulada'
          AND (vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_one(&estado.pool)
    .await?;

    let anuladas_por_motivo = sqlx::query!(
        r#"
        SELECT COALESCE(anulacion_motivo, 'Sin motivo') AS "motivo!",
               COUNT(*) AS "cantidad!",
               COALESCE(SUM(total_centavos), 0)::bigint AS "total_centavos!"
        FROM ventas.ventas
        WHERE estado = 'anulada'
          AND (vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY 1
        ORDER BY "total_centavos!" DESC
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    // Costo de lo vendido, a costo actual (misma aproximación que "Inventario":
    // no hay snapshot de costo en venta_items, así que el margen histórico se
    // recalcula con el costo de hoy, no el vigente al momento de la venta).
    let costo = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(ROUND(vi.cantidad * COALESCE(p.costo_actual_centavos, 0))), 0)::bigint AS "costo!"
        FROM ventas.venta_items vi
        JOIN ventas.ventas v ON v.id = vi.venta_id
        JOIN catalogo.productos p ON p.id = vi.producto_id
        WHERE v.estado = 'confirmada'
          AND (v.vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_one(&estado.pool)
    .await?;

    let margen_centavos = totales.facturado - costo;
    let margen_pct = if totales.facturado > 0 {
        (margen_centavos as f64 / totales.facturado as f64) * 100.0
    } else {
        0.0
    };

    let por_dia = sqlx::query!(
        r#"
        SELECT (vendida_en AT TIME ZONE $3)::date AS "fecha!",
               COALESCE(SUM(total_centavos), 0)::bigint AS "total!",
               COUNT(*) AS "tickets!"
        FROM ventas.ventas
        WHERE estado = 'confirmada'
          AND (vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY 1
        ORDER BY 1
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    let por_medio = sqlx::query!(
        r#"
        SELECT p.medio AS "medio: MedioPago",
               COALESCE(SUM(p.monto_centavos), 0)::bigint AS "total!"
        FROM ventas.pagos p
        JOIN ventas.ventas v ON v.id = p.venta_id
        WHERE v.estado = 'confirmada'
          AND (v.vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY p.medio
        ORDER BY 2 DESC
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    let ticket_promedio = if totales.tickets > 0 {
        totales.facturado / totales.tickets
    } else {
        0
    };

    Ok(Json(json!({
        "desde": desde,
        "hasta": hasta,
        "facturado_centavos": totales.facturado,
        "tickets": totales.tickets,
        "ticket_promedio_centavos": ticket_promedio,
        "descuentos_centavos": totales.descuentos,
        "costo_vendido_centavos": costo,
        "margen_centavos": margen_centavos,
        "margen_pct": margen_pct,
        "anuladas": anuladas.cantidad,
        "anuladas_centavos": anuladas.total_centavos,
        "anuladas_por_motivo": anuladas_por_motivo.into_iter().map(|a| json!({
            "motivo": a.motivo, "cantidad": a.cantidad, "total_centavos": a.total_centavos,
        })).collect::<Vec<_>>(),
        "por_dia": por_dia.into_iter().map(|d| json!({
            "fecha": d.fecha, "total_centavos": d.total, "tickets": d.tickets,
        })).collect::<Vec<_>>(),
        "por_medio": por_medio.into_iter().map(|m| json!({
            "medio": m.medio, "total_centavos": m.total,
        })).collect::<Vec<_>>(),
    })))
}

async fn top_productos(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();
    let limite = rango.limite.unwrap_or(10).clamp(1, 50);

    let filas = sqlx::query!(
        r#"
        SELECT vi.producto_id, vi.producto_nombre,
               SUM(vi.cantidad) AS "unidades!",
               COALESCE(SUM(vi.subtotal_centavos), 0)::bigint AS "facturado!"
        FROM ventas.venta_items vi
        JOIN ventas.ventas v ON v.id = vi.venta_id
        WHERE v.estado = 'confirmada'
          AND (v.vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY vi.producto_id, vi.producto_nombre
        ORDER BY "facturado!" DESC
        LIMIT $4
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!(filas.into_iter().map(|f| json!({
        "producto_id": f.producto_id,
        "nombre": f.producto_nombre,
        "unidades": f.unidades,
        "facturado_centavos": f.facturado,
    })).collect::<Vec<_>>())))
}

/// Contracara del top de ventas: productos con stock que no se movieron en el
/// período. Señal de capital inmovilizado para decisiones de compra/precio.
async fn productos_sin_movimiento(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();
    let limite = rango.limite.unwrap_or(15).clamp(1, 50);

    let filas = sqlx::query!(
        r#"
        SELECT p.id, p.nombre, s.cantidad AS "stock!",
               COALESCE(ROUND(s.cantidad * COALESCE(p.costo_actual_centavos, 0)), 0)::bigint AS "valor_centavos!"
        FROM catalogo.productos p
        JOIN inventario.stock_actual s ON s.producto_id = p.id
        WHERE p.activo AND s.cantidad > 0
          AND NOT EXISTS (
              SELECT 1 FROM ventas.venta_items vi
              JOIN ventas.ventas v ON v.id = vi.venta_id
              WHERE vi.producto_id = p.id AND v.estado = 'confirmada'
                AND (v.vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
          )
        ORDER BY "valor_centavos!" DESC
        LIMIT $4
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!(filas.into_iter().map(|f| json!({
        "producto_id": f.id,
        "nombre": f.nombre,
        "stock": f.stock,
        "valor_centavos": f.valor_centavos,
    })).collect::<Vec<_>>())))
}

/// Ranking de ventas/anulaciones/descuentos por operador: gestión de turnos y
/// señal de uso anómalo de permisos como anular_venta o aplicar_descuento.
async fn ventas_por_vendedor(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();

    let filas = sqlx::query!(
        r#"
        SELECT u.id, u.nombre,
               COUNT(*) FILTER (WHERE v.estado = 'confirmada') AS "tickets!",
               COALESCE(SUM(v.total_centavos) FILTER (WHERE v.estado = 'confirmada'), 0)::bigint AS "facturado!",
               COALESCE(SUM(v.descuento_centavos) FILTER (WHERE v.estado = 'confirmada'), 0)::bigint AS "descuentos!",
               COUNT(*) FILTER (WHERE v.estado = 'anulada') AS "anuladas!"
        FROM ventas.ventas v
        JOIN identidad.usuarios u ON u.id = v.usuario_id
        WHERE (v.vendida_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY u.id, u.nombre
        ORDER BY "facturado!" DESC
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!(filas.into_iter().map(|f| json!({
        "usuario_id": f.id,
        "nombre": f.nombre,
        "tickets": f.tickets,
        "facturado_centavos": f.facturado,
        "descuentos_centavos": f.descuentos,
        "anuladas": f.anuladas,
    })).collect::<Vec<_>>())))
}

/// Mermas: valor a costo de los ajustes negativos de stock (pérdida, rotura,
/// vencimiento, robo, conteo), agrupado por motivo. Los ajustes positivos
/// (sobrante de conteo) no son merma y quedan afuera.
async fn mermas(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();

    let filas = sqlx::query!(
        r#"
        SELECT a.motivo AS "motivo: MotivoAjuste",
               COALESCE(SUM(ROUND(-ms.cantidad * COALESCE(p.costo_actual_centavos, 0))), 0)::bigint AS "valor_centavos!"
        FROM inventario.movimientos_stock ms
        JOIN inventario.ajustes a ON a.id = ms.ajuste_id
        JOIN catalogo.productos p ON p.id = ms.producto_id
        WHERE ms.tipo = 'ajuste' AND ms.cantidad < 0
          AND (ms.creado_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY a.motivo
        ORDER BY "valor_centavos!" DESC
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    let total: i64 = filas.iter().map(|f| f.valor_centavos).sum();

    Ok(Json(json!({
        "total_centavos": total,
        "por_motivo": filas.into_iter().map(|f| json!({
            "motivo": f.motivo, "valor_centavos": f.valor_centavos,
        })).collect::<Vec<_>>(),
    })))
}

async fn fiado(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;

    let resumen = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(saldo_actual_centavos) FILTER (WHERE saldo_actual_centavos > 0), 0)::bigint AS "en_la_calle!",
               COUNT(*) FILTER (WHERE saldo_actual_centavos > 0) AS "deudores!"
        FROM clientes.clientes
        WHERE activo
        "#,
    )
    .fetch_one(&estado.pool)
    .await?;

    let top = sqlx::query!(
        r#"
        SELECT id, nombre, saldo_actual_centavos, limite_credito_centavos
        FROM clientes.clientes
        WHERE activo AND saldo_actual_centavos > 0
        ORDER BY saldo_actual_centavos DESC
        LIMIT 10
        "#,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "en_la_calle_centavos": resumen.en_la_calle,
        "deudores": resumen.deudores,
        "top_deudores": top.into_iter().map(|c| json!({
            "cliente_id": c.id,
            "nombre": c.nombre,
            "saldo_centavos": c.saldo_actual_centavos,
            "limite_centavos": c.limite_credito_centavos,
        })).collect::<Vec<_>>(),
    })))
}

async fn inventario(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;

    // Valor del inventario a costo y a precio de venta (solo stock positivo:
    // el negativo es un faltante a recalibrar, no valor).
    let valor = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(ROUND(s.cantidad * COALESCE(p.costo_actual_centavos, 0)))
                   FILTER (WHERE s.cantidad > 0), 0)::bigint AS "a_costo!",
               COALESCE(SUM(ROUND(s.cantidad * COALESCE(p.precio_actual_centavos, 0)))
                   FILTER (WHERE s.cantidad > 0), 0)::bigint AS "a_precio!",
               COUNT(*) FILTER (WHERE s.cantidad < 0) AS "con_stock_negativo!",
               COUNT(*) FILTER (WHERE s.cantidad > 0) AS "con_stock!"
        FROM inventario.stock_actual s
        JOIN catalogo.productos p ON p.id = s.producto_id
        WHERE s.cantidad <> 0
        "#,
    )
    .fetch_one(&estado.pool)
    .await?;

    let por_vencer = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "lotes!"
        FROM inventario.lotes
        WHERE cantidad_actual > 0 AND vencimiento <= CURRENT_DATE + 30
        "#,
    )
    .fetch_one(&estado.pool)
    .await?;

    // Detalle accionable: qué producto, cuánto y con qué urgencia, no solo el
    // conteo (que no alcanza para decidir qué liquidar primero).
    let proximos_vencimientos = sqlx::query!(
        r#"
        SELECT l.id, p.nombre AS producto_nombre, l.vencimiento, l.cantidad_actual AS "cantidad!",
               (l.vencimiento - CURRENT_DATE) AS "dias_restantes!"
        FROM inventario.lotes l
        JOIN catalogo.productos p ON p.id = l.producto_id
        WHERE l.cantidad_actual > 0 AND l.vencimiento <= CURRENT_DATE + 30
        ORDER BY l.vencimiento ASC
        LIMIT 15
        "#,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(json!({
        "valor_a_costo_centavos": valor.a_costo,
        "valor_a_precio_centavos": valor.a_precio,
        "productos_con_stock": valor.con_stock,
        "productos_con_stock_negativo": valor.con_stock_negativo,
        "lotes_por_vencer_30_dias": por_vencer,
        "proximos_vencimientos": proximos_vencimientos.into_iter().map(|l| json!({
            "lote_id": l.id,
            "producto_nombre": l.producto_nombre,
            "vencimiento": l.vencimiento,
            "cantidad": l.cantidad,
            "dias_restantes": l.dias_restantes,
        })).collect::<Vec<_>>(),
    })))
}

async fn arqueos(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let limite = rango.limite.unwrap_or(20).clamp(1, 100);

    let filas = sqlx::query!(
        r#"
        SELECT s.id, u.nombre AS usuario_nombre, s.abierta_en, s.cerrada_en,
               s.monto_contado_centavos, s.diferencia_arqueo_centavos
        FROM ventas.sesiones_caja s
        JOIN identidad.usuarios u ON u.id = s.usuario_id
        WHERE s.cerrada_en IS NOT NULL
        ORDER BY s.cerrada_en DESC
        LIMIT $1
        "#,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    // Acumulado de las sesiones listadas: una diferencia aislada es un evento,
    // un acumulado sostenido es una señal de un problema de manejo de caja.
    let total_diferencia: i64 = filas.iter().map(|s| s.diferencia_arqueo_centavos.unwrap_or(0)).sum();
    let con_diferencia = filas
        .iter()
        .filter(|s| s.diferencia_arqueo_centavos.unwrap_or(0) != 0)
        .count();

    Ok(Json(json!({
        "total_diferencia_centavos": total_diferencia,
        "con_diferencia": con_diferencia,
        "sesiones": filas.into_iter().map(|s| json!({
            "sesion_id": s.id,
            "usuario_nombre": s.usuario_nombre,
            "abierta_en": s.abierta_en,
            "cerrada_en": s.cerrada_en,
            "contado_centavos": s.monto_contado_centavos,
            "diferencia_centavos": s.diferencia_arqueo_centavos,
        })).collect::<Vec<_>>(),
    })))
}

async fn compras_resumen(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(rango): Query<RangoFechas>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let (desde, hasta) = rango.resolver();

    // Costo con IVA incluido (normalizado igual que el historial de precios).
    let filas = sqlx::query!(
        r#"
        SELECT COALESCE(p.nombre, 'Sin proveedor') AS "proveedor!",
               COUNT(DISTINCT r.id) AS "recepciones!",
               COALESCE(SUM(ROUND(
                   ri.cantidad * ri.costo_centavos *
                   CASE WHEN ri.costo_incluye_iva THEN 1 ELSE (1 + ri.iva_pct / 100) END
               )), 0)::bigint AS "total!"
        FROM compras.recepcion_items ri
        JOIN compras.recepciones r ON r.id = ri.recepcion_id
        LEFT JOIN compras.proveedores p ON p.id = r.proveedor_id
        WHERE r.estado <> 'borrador'
          AND (r.confirmada_en AT TIME ZONE $3)::date BETWEEN $1 AND $2
        GROUP BY p.nombre
        ORDER BY "total!" DESC
        "#,
        desde,
        hasta,
        ZONA_HORARIA,
    )
    .fetch_all(&estado.pool)
    .await?;

    let total: i64 = filas.iter().map(|f| f.total).sum();

    // Backlog operativo: recepciones cargadas pero nunca confirmadas, que si
    // no se muestran acá quedan fuera del radar de cualquiera.
    let pendientes_confirmar = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "pendientes!" FROM compras.recepciones WHERE estado = 'borrador'"#,
    )
    .fetch_one(&estado.pool)
    .await?;

    Ok(Json(json!({
        "desde": desde,
        "hasta": hasta,
        "total_comprado_centavos": total,
        "pendientes_confirmar": pendientes_confirmar,
        "por_proveedor": filas.into_iter().map(|f| json!({
            "proveedor": f.proveedor,
            "recepciones": f.recepciones,
            "total_centavos": f.total,
        })).collect::<Vec<_>>(),
    })))
}
