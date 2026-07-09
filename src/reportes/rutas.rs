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
use crate::reportes::ZONA_HORARIA;
use crate::ventas::MedioPago;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/ventas-resumen", get(ventas_resumen))
        .route("/top-productos", get(top_productos))
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

    let anuladas = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "anuladas!"
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
        "anuladas": anuladas,
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

    Ok(Json(json!({
        "valor_a_costo_centavos": valor.a_costo,
        "valor_a_precio_centavos": valor.a_precio,
        "productos_con_stock": valor.con_stock,
        "productos_con_stock_negativo": valor.con_stock_negativo,
        "lotes_por_vencer_30_dias": por_vencer,
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

    Ok(Json(json!(filas.into_iter().map(|s| json!({
        "sesion_id": s.id,
        "usuario_nombre": s.usuario_nombre,
        "abierta_en": s.abierta_en,
        "cerrada_en": s.cerrada_en,
        "contado_centavos": s.monto_contado_centavos,
        "diferencia_centavos": s.diferencia_arqueo_centavos,
    })).collect::<Vec<_>>())))
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

    Ok(Json(json!({
        "desde": desde,
        "hasta": hasta,
        "total_comprado_centavos": total,
        "por_proveedor": filas.into_iter().map(|f| json!({
            "proveedor": f.proveedor,
            "recepciones": f.recepciones,
            "total_centavos": f.total,
        })).collect::<Vec<_>>(),
    })))
}
