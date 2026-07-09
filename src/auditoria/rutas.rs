use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;

pub fn router() -> Router<Estado> {
    Router::new().route("/eventos", get(listar_eventos))
}

#[derive(Deserialize)]
struct FiltrosEventos {
    entidad: Option<String>,
    entidad_id: Option<Uuid>,
    accion: Option<String>,
    usuario_id: Option<Uuid>,
    desde: Option<NaiveDate>,
    hasta: Option<NaiveDate>,
    limite: Option<i64>,
    offset: Option<i64>,
}

#[derive(Serialize)]
struct EventoAuditoria {
    id: Uuid,
    entidad: String,
    entidad_id: Option<Uuid>,
    /// Nombre actual de la entidad afectada, resuelto según su tipo
    /// (puede ser NULL si la entidad no se puede resolver).
    entidad_nombre: Option<String>,
    accion: String,
    usuario_id: Option<Uuid>,
    /// Quién ejecutó la acción (NULL en eventos sin ejecutor, p. ej. un
    /// login fallido de un nombre inexistente).
    usuario_nombre: Option<String>,
    diff: Option<serde_json::Value>,
    creado_en: DateTime<Utc>,
}

async fn listar_eventos(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(filtros): Query<FiltrosEventos>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::VER_REPORTES)?;
    let limite = filtros.limite.unwrap_or(50).clamp(1, 200);
    let offset = filtros.offset.unwrap_or(0).max(0);

    let eventos = sqlx::query_as!(
        EventoAuditoria,
        r#"
        SELECT e.id, e.entidad, e.entidad_id,
               COALESCE(p.nombre, c.nombre, pr.nombre, cl.nombre, us.nombre, r.nombre)
                   AS "entidad_nombre?",
               e.accion, e.usuario_id, u.nombre AS "usuario_nombre?",
               e.diff, e.creado_en
        FROM auditoria.auditoria_eventos e
        LEFT JOIN identidad.usuarios u ON u.id = e.usuario_id
        LEFT JOIN catalogo.productos p ON e.entidad = 'producto' AND p.id = e.entidad_id
        LEFT JOIN catalogo.categorias c ON e.entidad = 'categoria' AND c.id = e.entidad_id
        LEFT JOIN compras.proveedores pr ON e.entidad = 'proveedor' AND pr.id = e.entidad_id
        LEFT JOIN clientes.clientes cl ON e.entidad = 'cliente' AND cl.id = e.entidad_id
        LEFT JOIN identidad.usuarios us ON e.entidad = 'usuario' AND us.id = e.entidad_id
        LEFT JOIN identidad.roles r ON e.entidad = 'rol' AND r.id = e.entidad_id
        WHERE ($1::text IS NULL OR e.entidad = $1)
          AND ($2::uuid IS NULL OR e.entidad_id = $2)
          AND ($3::text IS NULL OR e.accion = $3)
          AND ($4::uuid IS NULL OR e.usuario_id = $4)
          AND ($5::date IS NULL OR e.creado_en >= $5::date::timestamptz)
          AND ($6::date IS NULL OR e.creado_en < ($6::date + 1)::timestamptz)
        ORDER BY e.creado_en DESC
        LIMIT $7 OFFSET $8
        "#,
        filtros.entidad,
        filtros.entidad_id,
        filtros.accion,
        filtros.usuario_id,
        filtros.desde,
        filtros.hasta,
        limite + 1,
        offset,
    )
    .fetch_all(&estado.pool)
    .await?;

    // Se pide uno de más para saber si hay otra página.
    let hay_mas = eventos.len() as i64 > limite;
    let eventos: Vec<_> = eventos.into_iter().take(limite as usize).collect();

    Ok(Json(serde_json::json!({
        "eventos": eventos,
        "hay_mas": hay_mas,
        "offset": offset,
    })))
}
