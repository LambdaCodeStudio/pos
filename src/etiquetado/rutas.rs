//! Rutas /etiquetado/*, consumidas por la etiquetadora ESP32 (ver
//! `esp32.txt`). El dispositivo es tonto: escanea un código y muestra lo que
//! el backend le responde. Toda la inteligencia (resolver la recepción
//! activa, validar el producto, decidir si completa la recepción) vive acá.
//! Autenticación: middleware HMAC (`identidad::dispositivos::verificar_hmac`),
//! nunca JWT de usuario.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::etiquetado::formato_precio_centavos;
use crate::identidad::dispositivos::{self, DispositivoActual};
use crate::identidad::permisos;

// `from_fn` no soporta extraer `State` (ver doc de axum); el middleware
// necesita `State<Estado>` para buscar el dispositivo, así que se arma con
// `from_fn_with_state` y por eso este router recibe el `Estado` ya
// construido en vez de tomarlo por `.with_state()` como los demás módulos.
pub fn router(estado: Estado) -> Router<Estado> {
    Router::new()
        .route("/escanear", post(escanear))
        .route("/estado", get(estado_pendientes))
        .route_layer(axum::middleware::from_fn_with_state(
            estado,
            dispositivos::verificar_hmac,
        ))
}

#[derive(Deserialize)]
struct EscanearBody {
    codigo: String,
}

/// Siempre HTTP 200 con campo `estado` (salvo el 401 del middleware): el
/// firmware ya está escrito contra ese comportamiento y no distingue error
/// de transporte de resultado de negocio.
async fn escanear(
    State(estado): State<Estado>,
    Extension(dispositivo): Extension<DispositivoActual>,
    Json(datos): Json<EscanearBody>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    dispositivo.exigir(permisos::ETIQUETAR)?;

    let mut tx = estado.pool.begin().await?;

    // 1. Recepción activa: la confirmada más reciente con ítems pendientes.
    //    Se bloquea la fila para serializar escaneos concurrentes contra la
    //    misma recepción (evita una doble cuenta de "es el último pendiente").
    let recepcion = sqlx::query!(
        r#"
        SELECT r.id
        FROM compras.recepciones r
        WHERE r.estado = 'confirmada'
          AND EXISTS (
              SELECT 1 FROM compras.recepcion_items ri
              WHERE ri.recepcion_id = r.id AND NOT ri.etiquetado
          )
        ORDER BY r.confirmada_en DESC
        LIMIT 1
        FOR UPDATE OF r
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(recepcion) = recepcion else {
        return Ok(Json(json!({ "estado": "sin_recepcion_activa" })));
    };

    // 2. Producto por código de barras.
    let producto = sqlx::query!(
        r#"SELECT producto_id FROM catalogo.codigos_barras WHERE codigo = $1"#,
        datos.codigo,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(producto) = producto else {
        return Ok(Json(json!({ "estado": "codigo_desconocido" })));
    };

    // 3. ¿Ese producto tiene ítem en la recepción activa?
    let item = sqlx::query!(
        r#"
        SELECT ri.id, ri.etiquetado, ri.precio_final_centavos, pr.nombre AS producto_nombre
        FROM compras.recepcion_items ri
        JOIN catalogo.productos pr ON pr.id = ri.producto_id
        WHERE ri.recepcion_id = $1 AND ri.producto_id = $2
        FOR UPDATE OF ri
        "#,
        recepcion.id,
        producto.producto_id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(item) = item else {
        return Ok(Json(json!({ "estado": "no_en_recepcion" })));
    };

    // 4. Idempotencia: ya estaba etiquetado, no hay nada más que hacer.
    if item.etiquetado {
        return Ok(Json(json!({ "estado": "ya_etiquetado" })));
    }

    // Caso feliz: marcar etiquetado, atribuido al dispositivo.
    sqlx::query!(
        r#"
        UPDATE compras.recepcion_items
        SET etiquetado = true,
            etiquetado_en = now(),
            etiquetado_por_dispositivo_id = $2,
            actualizado_en = now()
        WHERE id = $1
        "#,
        item.id,
        dispositivo.id,
    )
    .execute(&mut *tx)
    .await?;

    let pendientes_restantes = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "n!"
        FROM compras.recepcion_items
        WHERE recepcion_id = $1 AND NOT etiquetado
        "#,
        recepcion.id,
    )
    .fetch_one(&mut *tx)
    .await?;

    // Si era el último pendiente, la recepción pasa a completada.
    if pendientes_restantes == 0 {
        sqlx::query!(
            r#"
            UPDATE compras.recepciones
            SET estado = 'completada', completada_en = now()
            WHERE id = $1
            "#,
            recepcion.id,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(Json(json!({
        "estado": "ok",
        "nombre": item.producto_nombre,
        "precio": formato_precio_centavos(item.precio_final_centavos),
        "codigo_barras": datos.codigo,
        "pendientes_restantes": pendientes_restantes,
    })))
}

async fn estado_pendientes(
    State(estado): State<Estado>,
    Extension(dispositivo): Extension<DispositivoActual>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    dispositivo.exigir(permisos::ETIQUETAR)?;

    let recepcion = sqlx::query!(
        r#"
        SELECT r.id
        FROM compras.recepciones r
        WHERE r.estado = 'confirmada'
          AND EXISTS (
              SELECT 1 FROM compras.recepcion_items ri
              WHERE ri.recepcion_id = r.id AND NOT ri.etiquetado
          )
        ORDER BY r.confirmada_en DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&estado.pool)
    .await?;

    let Some(recepcion) = recepcion else {
        return Ok(Json(json!({ "pendientes": -1 })));
    };

    let pendientes = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "n!"
        FROM compras.recepcion_items
        WHERE recepcion_id = $1 AND NOT etiquetado
        "#,
        recepcion.id,
    )
    .fetch_one(&estado.pool)
    .await?;

    Ok(Json(json!({ "pendientes": pendientes })))
}
