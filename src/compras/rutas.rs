use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auditoria;
use crate::catalogo;
use crate::compras::precio::{
    calcular_precio_final_centavos, normalizar_costo_con_iva_centavos,
    redondear_a_multiplo_centavos,
};
use crate::compras::EstadoRecepcion;
use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;
use crate::inventario;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/proveedores", get(listar_proveedores).post(crear_proveedor))
        .route(
            "/proveedores/{id}",
            axum::routing::patch(actualizar_proveedor).delete(desactivar_proveedor),
        )
        .route("/recepciones", get(listar_recepciones).post(crear_recepcion))
        .route("/recepciones/{id}", get(obtener_recepcion))
        .route("/recepciones/{id}/items", put(cargar_item))
        .route("/recepciones/{id}/items/{producto_id}", axum::routing::delete(quitar_item))
        .route("/recepciones/{id}/confirmar", post(confirmar_recepcion))
}

// ---------- Proveedores ----------

#[derive(Serialize)]
struct Proveedor {
    id: Uuid,
    nombre: String,
    cuit: Option<String>,
    telefono: Option<String>,
    precios_con_iva: bool,
    condiciones_pago: Option<String>,
    activo: bool,
}

#[derive(Deserialize)]
struct FiltroInactivos {
    incluir_inactivos: Option<bool>,
}

async fn listar_proveedores(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroInactivos>,
) -> Result<Json<Vec<Proveedor>>, ErrorApi> {
    let incluir_inactivos = filtro.incluir_inactivos.unwrap_or(false);
    let proveedores = sqlx::query_as!(
        Proveedor,
        r#"
        SELECT id, nombre, cuit, telefono, precios_con_iva, condiciones_pago, activo
        FROM compras.proveedores
        WHERE activo OR $1
        ORDER BY nombre
        "#,
        incluir_inactivos,
    )
    .fetch_all(&estado.pool)
    .await?;
    Ok(Json(proveedores))
}

#[derive(Deserialize)]
struct CrearProveedor {
    nombre: String,
    cuit: Option<String>,
    telefono: Option<String>,
    precios_con_iva: Option<bool>,
    condiciones_pago: Option<String>,
}

async fn crear_proveedor(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearProveedor>,
) -> Result<Json<Proveedor>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_PROVEEDORES)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }

    let id = Uuid::now_v7();
    let precios_con_iva = datos.precios_con_iva.unwrap_or(true);
    let mut tx = estado.pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO compras.proveedores (id, nombre, cuit, telefono, precios_con_iva, condiciones_pago)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        datos.nombre.trim(),
        datos.cuit,
        datos.telefono,
        precios_con_iva,
        datos.condiciones_pago,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "proveedor",
        Some(id),
        "crear",
        Some(usuario.id),
        Some(json!({ "nombre": datos.nombre.trim(), "precios_con_iva": precios_con_iva })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(Proveedor {
        id,
        nombre: datos.nombre.trim().to_string(),
        cuit: datos.cuit,
        telefono: datos.telefono,
        precios_con_iva,
        condiciones_pago: datos.condiciones_pago,
        activo: true,
    }))
}

#[derive(Deserialize)]
struct ActualizarProveedor {
    nombre: Option<String>,
    cuit: Option<String>,
    telefono: Option<String>,
    precios_con_iva: Option<bool>,
    condiciones_pago: Option<String>,
    activo: Option<bool>,
}

async fn actualizar_proveedor(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarProveedor>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_PROVEEDORES)?;

    let mut tx = estado.pool.begin().await?;
    let antes = sqlx::query!(
        r#"SELECT nombre, cuit, telefono, precios_con_iva, condiciones_pago, activo
           FROM compras.proveedores WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    sqlx::query!(
        r#"
        UPDATE compras.proveedores SET
            nombre = COALESCE($2, nombre),
            cuit = COALESCE($3, cuit),
            telefono = COALESCE($4, telefono),
            precios_con_iva = COALESCE($5, precios_con_iva),
            condiciones_pago = COALESCE($6, condiciones_pago),
            activo = COALESCE($7, activo),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        datos.cuit,
        datos.telefono,
        datos.precios_con_iva,
        datos.condiciones_pago,
        datos.activo,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "proveedor",
        Some(id),
        "actualizar",
        Some(usuario.id),
        Some(auditoria::diff_antes_despues(
            json!({
                "nombre": antes.nombre, "cuit": antes.cuit, "telefono": antes.telefono,
                "precios_con_iva": antes.precios_con_iva,
                "condiciones_pago": antes.condiciones_pago, "activo": antes.activo,
            }),
            json!({
                "nombre": datos.nombre, "cuit": datos.cuit, "telefono": datos.telefono,
                "precios_con_iva": datos.precios_con_iva,
                "condiciones_pago": datos.condiciones_pago, "activo": datos.activo,
            }),
        )),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn desactivar_proveedor(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_PROVEEDORES)?;
    let mut tx = estado.pool.begin().await?;
    let resultado = sqlx::query!(
        r#"UPDATE compras.proveedores SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;
    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "proveedor", Some(id), "desactivar", Some(usuario.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Recepciones ----------

#[derive(Serialize)]
struct RecepcionResumen {
    id: Uuid,
    proveedor_id: Option<Uuid>,
    proveedor_nombre: Option<String>,
    estado: EstadoRecepcion,
    observaciones: Option<String>,
    creada_en: DateTime<Utc>,
    confirmada_en: Option<DateTime<Utc>>,
    completada_en: Option<DateTime<Utc>>,
    cantidad_items: i64,
    items_pendientes_etiquetar: i64,
}

#[derive(Deserialize)]
struct CrearRecepcion {
    /// UUID generado por el cliente para idempotencia offline-first.
    id: Option<Uuid>,
    proveedor_id: Option<Uuid>,
    observaciones: Option<String>,
}

async fn crear_recepcion(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearRecepcion>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::CONFIRMAR_RECEPCION)?;

    let id = datos.id.unwrap_or_else(Uuid::now_v7);

    if let Some(proveedor_id) = datos.proveedor_id {
        sqlx::query!(r#"SELECT id FROM compras.proveedores WHERE id = $1 AND activo"#, proveedor_id)
            .fetch_optional(&estado.pool)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("proveedor inexistente o inactivo".into()))?;
    }

    // Idempotente: reintentar con el mismo UUID no duplica nada.
    let fila = sqlx::query!(
        r#"
        INSERT INTO compras.recepciones (id, proveedor_id, observaciones, usuario_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        id,
        datos.proveedor_id,
        datos.observaciones,
        usuario.id,
    )
    .fetch_optional(&estado.pool)
    .await?;

    let recien_creada = fila.is_some();
    let rec = sqlx::query!(
        r#"SELECT id, estado AS "estado: EstadoRecepcion" FROM compras.recepciones WHERE id = $1"#,
        id,
    )
    .fetch_one(&estado.pool)
    .await?;

    Ok(Json(json!({
        "id": rec.id,
        "estado": rec.estado,
        "creada": recien_creada,
    })))
}

#[derive(Deserialize)]
struct FiltroRecepciones {
    estado: Option<EstadoRecepcion>,
    limite: Option<i64>,
}

async fn listar_recepciones(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroRecepciones>,
) -> Result<Json<Vec<RecepcionResumen>>, ErrorApi> {
    let limite = filtro.limite.unwrap_or(50).clamp(1, 200);
    let filas = sqlx::query!(
        r#"
        SELECT r.id, r.proveedor_id, p.nombre AS "proveedor_nombre?",
               r.estado AS "estado: EstadoRecepcion",
               r.observaciones, r.creada_en, r.confirmada_en, r.completada_en,
               COUNT(ri.id) AS "cantidad_items!",
               COUNT(ri.id) FILTER (WHERE NOT ri.etiquetado) AS "items_pendientes_etiquetar!"
        FROM compras.recepciones r
        LEFT JOIN compras.proveedores p ON p.id = r.proveedor_id
        LEFT JOIN compras.recepcion_items ri ON ri.recepcion_id = r.id
        WHERE ($1::compras.estado_recepcion IS NULL OR r.estado = $1)
        GROUP BY r.id, r.proveedor_id, p.nombre, r.estado, r.observaciones,
                 r.creada_en, r.confirmada_en, r.completada_en
        ORDER BY r.creada_en DESC
        LIMIT $2
        "#,
        filtro.estado as Option<EstadoRecepcion>,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| RecepcionResumen {
                id: f.id,
                proveedor_id: f.proveedor_id,
                proveedor_nombre: f.proveedor_nombre,
                estado: f.estado,
                observaciones: f.observaciones,
                creada_en: f.creada_en,
                confirmada_en: f.confirmada_en,
                completada_en: f.completada_en,
                cantidad_items: f.cantidad_items,
                items_pendientes_etiquetar: f.items_pendientes_etiquetar,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct ItemRecepcion {
    id: Uuid,
    producto_id: Uuid,
    producto_nombre: String,
    cantidad: Decimal,
    costo_centavos: i64,
    costo_incluye_iva: bool,
    iva_pct: Decimal,
    markup_pct: Decimal,
    precio_final_centavos: i64,
    vencimiento: Option<NaiveDate>,
    etiquetado: bool,
    etiquetado_en: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct RecepcionDetalle {
    id: Uuid,
    proveedor_id: Option<Uuid>,
    proveedor_nombre: Option<String>,
    estado: EstadoRecepcion,
    observaciones: Option<String>,
    creada_en: DateTime<Utc>,
    confirmada_en: Option<DateTime<Utc>>,
    completada_en: Option<DateTime<Utc>>,
    items: Vec<ItemRecepcion>,
}

async fn obtener_recepcion(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<RecepcionDetalle>, ErrorApi> {
    let cab = sqlx::query!(
        r#"
        SELECT r.id, r.proveedor_id, p.nombre AS "proveedor_nombre?",
               r.estado AS "estado: EstadoRecepcion",
               r.observaciones, r.creada_en, r.confirmada_en, r.completada_en
        FROM compras.recepciones r
        LEFT JOIN compras.proveedores p ON p.id = r.proveedor_id
        WHERE r.id = $1
        "#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let items = sqlx::query!(
        r#"
        SELECT ri.id, ri.producto_id, pr.nombre AS producto_nombre, ri.cantidad,
               ri.costo_centavos, ri.costo_incluye_iva, ri.iva_pct, ri.markup_pct,
               ri.precio_final_centavos, ri.vencimiento, ri.etiquetado, ri.etiquetado_en
        FROM compras.recepcion_items ri
        JOIN catalogo.productos pr ON pr.id = ri.producto_id
        WHERE ri.recepcion_id = $1
        ORDER BY ri.creado_en
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(RecepcionDetalle {
        id: cab.id,
        proveedor_id: cab.proveedor_id,
        proveedor_nombre: cab.proveedor_nombre,
        estado: cab.estado,
        observaciones: cab.observaciones,
        creada_en: cab.creada_en,
        confirmada_en: cab.confirmada_en,
        completada_en: cab.completada_en,
        items: items
            .into_iter()
            .map(|i| ItemRecepcion {
                id: i.id,
                producto_id: i.producto_id,
                producto_nombre: i.producto_nombre,
                cantidad: i.cantidad,
                costo_centavos: i.costo_centavos,
                costo_incluye_iva: i.costo_incluye_iva,
                iva_pct: i.iva_pct,
                markup_pct: i.markup_pct,
                precio_final_centavos: i.precio_final_centavos,
                vencimiento: i.vencimiento,
                etiquetado: i.etiquetado,
                etiquetado_en: i.etiquetado_en,
            })
            .collect(),
    }))
}

// ---------- Carga de ítems ----------

#[derive(Deserialize)]
struct CargarItem {
    producto_id: Uuid,
    cantidad: Decimal,
    costo_centavos: i64,
    /// Default: precios_con_iva del proveedor (o true si la recepción no tiene proveedor).
    costo_incluye_iva: Option<bool>,
    /// Cascada: valor explícito → override del producto → default de la categoría.
    iva_pct: Option<Decimal>,
    markup_pct: Option<Decimal>,
    vencimiento: Option<NaiveDate>,
}

async fn cargar_item(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(recepcion_id): Path<Uuid>,
    Json(datos): Json<CargarItem>,
) -> Result<Json<ItemRecepcion>, ErrorApi> {
    usuario.exigir(permisos::CONFIRMAR_RECEPCION)?;
    if datos.cantidad <= Decimal::ZERO {
        return Err(ErrorApi::Validacion("la cantidad debe ser mayor a cero".into()));
    }
    if datos.costo_centavos < 0 {
        return Err(ErrorApi::Validacion("el costo no puede ser negativo".into()));
    }

    let mut tx = estado.pool.begin().await?;

    let rec = sqlx::query!(
        r#"
        SELECT r.estado AS "estado: EstadoRecepcion", p.precios_con_iva AS "precios_con_iva?"
        FROM compras.recepciones r
        LEFT JOIN compras.proveedores p ON p.id = r.proveedor_id
        WHERE r.id = $1
        FOR UPDATE OF r
        "#,
        recepcion_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    if rec.estado != EstadoRecepcion::Borrador {
        return Err(ErrorApi::Conflicto(
            "solo se pueden cargar ítems en una recepción en borrador".into(),
        ));
    }

    let producto = catalogo::producto_para_compra(&mut *tx, datos.producto_id).await?;

    if producto.controla_vencimiento && datos.vencimiento.is_none() {
        return Err(ErrorApi::Validacion(format!(
            "el producto \"{}\" controla vencimiento: la fecha es obligatoria",
            producto.nombre
        )));
    }

    let costo_incluye_iva = datos
        .costo_incluye_iva
        .or(rec.precios_con_iva)
        .unwrap_or(true);
    let iva_pct = datos.iva_pct.unwrap_or(producto.iva_pct);
    let markup_pct = datos.markup_pct.unwrap_or(producto.markup_pct);
    // Redondeo comercial configurable (p. ej. al múltiplo de $100): facilita
    // los totales del ticket y el manejo de efectivo en el mostrador.
    let redondeo = catalogo::redondeo_precio_configurado(&mut *tx).await?;
    let precio_final_centavos = redondear_a_multiplo_centavos(
        calcular_precio_final_centavos(datos.costo_centavos, costo_incluye_iva, iva_pct, markup_pct)?,
        redondeo,
    );

    // Carga idempotente: reintentos o correcciones del mismo producto pisan
    // el ítem existente (UNIQUE recepcion_id, producto_id) sin duplicar.
    let fila = sqlx::query!(
        r#"
        INSERT INTO compras.recepcion_items
            (id, recepcion_id, producto_id, cantidad, costo_centavos, costo_incluye_iva,
             iva_pct, markup_pct, precio_final_centavos, vencimiento, usuario_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (recepcion_id, producto_id) DO UPDATE SET
            cantidad = EXCLUDED.cantidad,
            costo_centavos = EXCLUDED.costo_centavos,
            costo_incluye_iva = EXCLUDED.costo_incluye_iva,
            iva_pct = EXCLUDED.iva_pct,
            markup_pct = EXCLUDED.markup_pct,
            precio_final_centavos = EXCLUDED.precio_final_centavos,
            vencimiento = EXCLUDED.vencimiento,
            usuario_id = EXCLUDED.usuario_id,
            actualizado_en = now()
        RETURNING id
        "#,
        Uuid::now_v7(),
        recepcion_id,
        datos.producto_id,
        datos.cantidad,
        datos.costo_centavos,
        costo_incluye_iva,
        iva_pct,
        markup_pct,
        precio_final_centavos,
        datos.vencimiento,
        usuario.id,
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(ItemRecepcion {
        id: fila.id,
        producto_id: datos.producto_id,
        producto_nombre: producto.nombre,
        cantidad: datos.cantidad,
        costo_centavos: datos.costo_centavos,
        costo_incluye_iva,
        iva_pct,
        markup_pct,
        precio_final_centavos,
        vencimiento: datos.vencimiento,
        etiquetado: false,
        etiquetado_en: None,
    }))
}

async fn quitar_item(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path((recepcion_id, producto_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::CONFIRMAR_RECEPCION)?;

    let mut tx = estado.pool.begin().await?;
    let rec = sqlx::query!(
        r#"SELECT estado AS "estado: EstadoRecepcion" FROM compras.recepciones
           WHERE id = $1 FOR UPDATE"#,
        recepcion_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    if rec.estado != EstadoRecepcion::Borrador {
        return Err(ErrorApi::Conflicto(
            "solo se pueden quitar ítems de una recepción en borrador".into(),
        ));
    }

    sqlx::query!(
        r#"DELETE FROM compras.recepcion_items WHERE recepcion_id = $1 AND producto_id = $2"#,
        recepcion_id,
        producto_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- LA TRANSACCIÓN CRÍTICA ----------

/// Confirmar recepción: atómica (todo o nada), con lock FOR UPDATE sobre la
/// recepción, idempotente si ya está confirmada o completada.
/// Por cada ítem: ledger de precios + proyecciones del producto, ledger de
/// inventario (+ lote si controla vencimiento) + proyección de stock.
async fn confirmar_recepcion(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::CONFIRMAR_RECEPCION)?;

    let mut tx = estado.pool.begin().await?;

    let rec = sqlx::query!(
        r#"SELECT estado AS "estado: EstadoRecepcion" FROM compras.recepciones
           WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    // Idempotencia: reintentar la confirmación de una recepción ya
    // confirmada/completada es un no-op exitoso.
    if rec.estado != EstadoRecepcion::Borrador {
        return Ok(Json(json!({ "id": id, "estado": rec.estado, "ya_estaba_confirmada": true })));
    }

    let items = sqlx::query!(
        r#"
        SELECT ri.id, ri.producto_id, pr.nombre AS producto_nombre, ri.cantidad,
               ri.costo_centavos, ri.costo_incluye_iva, ri.iva_pct, ri.markup_pct,
               ri.precio_final_centavos, ri.vencimiento, pr.controla_vencimiento,
               pr.precio_actual_centavos AS precio_anterior_centavos
        FROM compras.recepcion_items ri
        JOIN catalogo.productos pr ON pr.id = ri.producto_id
        WHERE ri.recepcion_id = $1
        ORDER BY ri.creado_en
        "#,
        id,
    )
    .fetch_all(&mut *tx)
    .await?;

    if items.is_empty() {
        return Err(ErrorApi::Validacion(
            "no se puede confirmar una recepción sin ítems".into(),
        ));
    }
    for item in &items {
        if item.controla_vencimiento && item.vencimiento.is_none() {
            return Err(ErrorApi::Validacion(format!(
                "el producto \"{}\" controla vencimiento y su ítem no tiene fecha",
                item.producto_nombre
            )));
        }
    }

    for item in &items {
        // 1. Ledger de precios + proyecciones de precio/costo del producto.
        let costo_normalizado = normalizar_costo_con_iva_centavos(
            item.costo_centavos,
            item.costo_incluye_iva,
            item.iva_pct,
        )?;

        sqlx::query!(
            r#"
            INSERT INTO catalogo.precios_historial
                (id, producto_id, precio_centavos, costo_centavos, recepcion_id, usuario_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            Uuid::now_v7(),
            item.producto_id,
            item.precio_final_centavos,
            costo_normalizado,
            id,
            usuario.id,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE catalogo.productos
            SET precio_actual_centavos = $2, costo_actual_centavos = $3, actualizado_en = now()
            WHERE id = $1
            "#,
            item.producto_id,
            item.precio_final_centavos,
            costo_normalizado,
        )
        .execute(&mut *tx)
        .await?;

        crate::clientes::reindexar_precio_producto(
            &mut tx,
            item.producto_id,
            item.precio_anterior_centavos,
            item.precio_final_centavos,
            usuario.id,
        )
        .await?;

        // 2. Ledger de inventario: entrada al depósito principal, con lote
        //    si el producto controla vencimiento.
        let vencimiento_lote = if item.controla_vencimiento {
            item.vencimiento
        } else {
            None
        };
        inventario::registrar_entrada_recepcion(
            &mut tx,
            item.producto_id,
            item.cantidad,
            item.id,
            vencimiento_lote,
            usuario.id,
        )
        .await?;
    }

    // 3. Transición de estado. Los ítems quedan etiquetado = false: trabajo
    //    pendiente para el recorrido físico de etiquetado.
    sqlx::query!(
        r#"
        UPDATE compras.recepciones
        SET estado = 'confirmada', confirmada_en = now(), confirmada_por = $2
        WHERE id = $1
        "#,
        id,
        usuario.id,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "id": id,
        "estado": EstadoRecepcion::Confirmada,
        "items_confirmados": items.len(),
    })))
}

// El flujo de etiquetado (marcar ítem, completar la recepción cuando no
// queda ningún pendiente) vive ahora en `crate::etiquetado`, bajo el
// contrato HMAC del dispositivo (ver esa rutas.rs para el reemplazo de los
// endpoints que estaban acá: GET .../etiquetas-pendientes y POST
// .../items/{item_id}/etiquetar).
