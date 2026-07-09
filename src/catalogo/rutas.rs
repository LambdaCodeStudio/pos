use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auditoria;
use crate::catalogo::UnidadDeVenta;
use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::UsuarioActual;
use crate::identidad::permisos;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/categorias", get(listar_categorias).post(crear_categoria))
        .route(
            "/categorias/{id}",
            axum::routing::patch(actualizar_categoria).delete(desactivar_categoria),
        )
        .route("/productos", get(buscar_productos).post(crear_producto))
        .route(
            "/productos/{id}",
            get(obtener_producto)
                .patch(actualizar_producto)
                .delete(desactivar_producto),
        )
        .route("/productos/{id}/codigos-barras", post(agregar_codigo_barras))
        .route("/productos/{id}/precio", post(cambiar_precio_manual))
        .route("/productos/{id}/precios", get(historial_precios))
        .route(
            "/codigos-barras/{codigo}",
            get(resolver_codigo_barras).delete(quitar_codigo_barras),
        )
        .route("/sincronizacion-caja", get(sincronizacion_caja))
        .route("/configuracion", get(obtener_configuracion).put(actualizar_configuracion))
}

// ---------- Configuración global ----------

#[derive(Serialize)]
struct Configuracion {
    /// Redondeo comercial del precio calculado en recepciones, en centavos
    /// (0 = sin redondeo; 10000 = al múltiplo de $100 más cercano).
    redondeo_precio_centavos: i64,
}

async fn obtener_configuracion(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
) -> Result<Json<Configuracion>, ErrorApi> {
    Ok(Json(Configuracion {
        redondeo_precio_centavos: crate::catalogo::redondeo_precio_configurado(&estado.pool).await?,
    }))
}

#[derive(Deserialize)]
struct ActualizarConfiguracion {
    redondeo_precio_centavos: i64,
}

async fn actualizar_configuracion(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<ActualizarConfiguracion>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::MODIFICAR_PRECIOS)?;
    if !(0..=1_000_000).contains(&datos.redondeo_precio_centavos) {
        return Err(ErrorApi::Validacion(
            "el redondeo debe estar entre 0 (sin redondeo) y $10.000".into(),
        ));
    }

    let mut tx = estado.pool.begin().await?;
    sqlx::query!(
        r#"
        INSERT INTO catalogo.configuracion (clave, valor, actualizado_en)
        VALUES ('redondeo_precio_centavos', $1, now())
        ON CONFLICT (clave) DO UPDATE SET valor = EXCLUDED.valor, actualizado_en = now()
        "#,
        json!(datos.redondeo_precio_centavos),
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "configuracion",
        None,
        "actualizar",
        Some(usuario.id),
        Some(json!({ "redondeo_precio_centavos": datos.redondeo_precio_centavos })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(json!({ "redondeo_precio_centavos": datos.redondeo_precio_centavos })))
}

/// Volcado del catálogo vendible para el caché offline de la PWA de caja:
/// todos los productos activos con precio e IVA resueltos y sus códigos.
/// Sin límite: la caja necesita el catálogo completo para operar sin red.
async fn sincronizacion_caja(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let filas = sqlx::query!(
        r#"
        SELECT p.id, p.nombre,
               p.unidad_de_venta AS "unidad_de_venta: UnidadDeVenta",
               p.precio_actual_centavos,
               COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct!",
               COALESCE(array_agg(cb.codigo) FILTER (WHERE cb.codigo IS NOT NULL), '{}') AS "codigos_barras!"
        FROM catalogo.productos p
        JOIN catalogo.categorias c ON c.id = p.categoria_id
        LEFT JOIN catalogo.codigos_barras cb ON cb.producto_id = p.id
        WHERE p.activo
        GROUP BY p.id, p.nombre, p.unidad_de_venta, p.precio_actual_centavos,
                 p.iva_pct_override, c.iva_pct
        ORDER BY p.nombre
        "#,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(serde_json::json!({
        "generado_en": chrono::Utc::now(),
        "productos": filas.into_iter().map(|f| serde_json::json!({
            "id": f.id,
            "nombre": f.nombre,
            "unidad_de_venta": f.unidad_de_venta,
            "precio_actual_centavos": f.precio_actual_centavos,
            "iva_pct": f.iva_pct,
            "codigos_barras": f.codigos_barras,
        })).collect::<Vec<_>>(),
    })))
}

// ---------- Categorías ----------

#[derive(Serialize)]
struct Categoria {
    id: Uuid,
    nombre: String,
    padre_id: Option<Uuid>,
    markup_pct: Decimal,
    iva_pct: Decimal,
    activo: bool,
}

#[derive(Deserialize)]
struct FiltroInactivos {
    incluir_inactivos: Option<bool>,
}

async fn listar_categorias(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroInactivos>,
) -> Result<Json<Vec<Categoria>>, ErrorApi> {
    let incluir_inactivos = filtro.incluir_inactivos.unwrap_or(false);
    let categorias = sqlx::query_as!(
        Categoria,
        r#"
        SELECT id, nombre, padre_id, markup_pct, iva_pct, activo
        FROM catalogo.categorias
        WHERE activo OR $1
        ORDER BY nombre
        "#,
        incluir_inactivos,
    )
    .fetch_all(&estado.pool)
    .await?;
    Ok(Json(categorias))
}

#[derive(Deserialize)]
struct CrearCategoria {
    nombre: String,
    padre_id: Option<Uuid>,
    markup_pct: Option<Decimal>,
    iva_pct: Option<Decimal>,
}

fn validar_pct(nombre: &str, valor: Decimal) -> Result<(), ErrorApi> {
    if valor < Decimal::ZERO || valor > Decimal::from(999) {
        return Err(ErrorApi::Validacion(format!("{nombre} fuera de rango")));
    }
    Ok(())
}

async fn crear_categoria(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearCategoria>,
) -> Result<Json<Categoria>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }
    let markup = datos.markup_pct.unwrap_or(Decimal::new(4000, 2));
    let iva = datos.iva_pct.unwrap_or(Decimal::new(2100, 2));
    validar_pct("markup_pct", markup)?;
    validar_pct("iva_pct", iva)?;

    let id = Uuid::now_v7();
    let mut tx = estado.pool.begin().await?;

    if let Some(padre_id) = datos.padre_id {
        sqlx::query!(r#"SELECT id FROM catalogo.categorias WHERE id = $1 AND activo"#, padre_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("categoría padre inexistente o inactiva".into()))?;
    }

    let insertada = sqlx::query!(
        r#"
        INSERT INTO catalogo.categorias (id, nombre, padre_id, markup_pct, iva_pct)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (nombre) DO NOTHING
        RETURNING id
        "#,
        id,
        datos.nombre.trim(),
        datos.padre_id,
        markup,
        iva,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if insertada.is_none() {
        return Err(ErrorApi::Conflicto("ya existe una categoría con ese nombre".into()));
    }

    auditoria::registrar(
        &mut *tx,
        "categoria",
        Some(id),
        "crear",
        Some(usuario.id),
        Some(json!({ "nombre": datos.nombre.trim(), "markup_pct": markup, "iva_pct": iva })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(Categoria {
        id,
        nombre: datos.nombre.trim().to_string(),
        padre_id: datos.padre_id,
        markup_pct: markup,
        iva_pct: iva,
        activo: true,
    }))
}

#[derive(Deserialize)]
struct ActualizarCategoria {
    nombre: Option<String>,
    #[serde(default)]
    padre_id: Option<Option<Uuid>>,
    markup_pct: Option<Decimal>,
    iva_pct: Option<Decimal>,
}

async fn actualizar_categoria(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarCategoria>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    if let Some(v) = datos.markup_pct {
        validar_pct("markup_pct", v)?;
    }
    if let Some(v) = datos.iva_pct {
        validar_pct("iva_pct", v)?;
    }

    let mut tx = estado.pool.begin().await?;
    let antes = sqlx::query!(
        r#"SELECT nombre, padre_id, markup_pct, iva_pct FROM catalogo.categorias
           WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    let (cambiar_padre, nuevo_padre) = match datos.padre_id {
        Some(nuevo) => (true, nuevo),
        None => (false, None),
    };
    if let Some(padre_id) = nuevo_padre {
        if padre_id == id {
            return Err(ErrorApi::Validacion("una categoría no puede ser su propio padre".into()));
        }
        sqlx::query!(r#"SELECT id FROM catalogo.categorias WHERE id = $1 AND activo"#, padre_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("categoría padre inexistente o inactiva".into()))?;
    }

    sqlx::query!(
        r#"
        UPDATE catalogo.categorias SET
            nombre = COALESCE($2, nombre),
            padre_id = CASE WHEN $3 THEN $4 ELSE padre_id END,
            markup_pct = COALESCE($5, markup_pct),
            iva_pct = COALESCE($6, iva_pct),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        cambiar_padre,
        nuevo_padre,
        datos.markup_pct,
        datos.iva_pct,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "categoria",
        Some(id),
        "actualizar",
        Some(usuario.id),
        Some(auditoria::diff_antes_despues(
            json!({
                "nombre": antes.nombre, "padre_id": antes.padre_id,
                "markup_pct": antes.markup_pct, "iva_pct": antes.iva_pct,
            }),
            json!({
                "nombre": datos.nombre, "padre_id": datos.padre_id,
                "markup_pct": datos.markup_pct, "iva_pct": datos.iva_pct,
            }),
        )),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn desactivar_categoria(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;

    let mut tx = estado.pool.begin().await?;
    let en_uso = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM catalogo.productos WHERE categoria_id = $1 AND activo) AS "en_uso!""#,
        id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if en_uso {
        return Err(ErrorApi::Conflicto(
            "hay productos activos en esta categoría; movelos antes de desactivarla".into(),
        ));
    }

    let resultado = sqlx::query!(
        r#"UPDATE catalogo.categorias SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;
    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "categoria", Some(id), "desactivar", Some(usuario.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Productos ----------

#[derive(Serialize)]
struct ProductoDetalle {
    id: Uuid,
    nombre: String,
    categoria_id: Uuid,
    categoria_nombre: String,
    markup_pct_override: Option<Decimal>,
    iva_pct_override: Option<Decimal>,
    markup_pct_resuelto: Decimal,
    iva_pct_resuelto: Decimal,
    unidad_de_venta: UnidadDeVenta,
    controla_vencimiento: bool,
    precio_actual_centavos: Option<i64>,
    costo_actual_centavos: Option<i64>,
    activo: bool,
    codigos_barras: Vec<String>,
}

#[derive(Deserialize)]
struct FiltroProductos {
    buscar: Option<String>,
    incluir_inactivos: Option<bool>,
    limite: Option<i64>,
}

async fn buscar_productos(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Query(filtro): Query<FiltroProductos>,
) -> Result<Json<Vec<ProductoDetalle>>, ErrorApi> {
    let incluir_inactivos = filtro.incluir_inactivos.unwrap_or(false);
    let limite = filtro.limite.unwrap_or(30).clamp(1, 200);
    let buscar = filtro
        .buscar
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Autocompletado tolerante a errores de tipeo: ILIKE para prefijos/substrings
    // exactos + similitud de trigramas (pg_trgm) para typos.
    let filas = sqlx::query!(
        r#"
        SELECT p.id, p.nombre, p.categoria_id, c.nombre AS categoria_nombre,
               p.markup_pct_override, p.iva_pct_override,
               COALESCE(p.markup_pct_override, c.markup_pct) AS "markup_pct_resuelto!",
               COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct_resuelto!",
               p.unidad_de_venta AS "unidad_de_venta: UnidadDeVenta",
               p.controla_vencimiento, p.precio_actual_centavos, p.costo_actual_centavos,
               p.activo,
               COALESCE(array_agg(cb.codigo) FILTER (WHERE cb.codigo IS NOT NULL), '{}') AS "codigos_barras!"
        FROM catalogo.productos p
        JOIN catalogo.categorias c ON c.id = p.categoria_id
        LEFT JOIN catalogo.codigos_barras cb ON cb.producto_id = p.id
        WHERE (p.activo OR $2)
          AND ($1::text IS NULL OR p.nombre ILIKE '%' || $1 || '%' OR similarity(p.nombre, $1) > 0.25)
        GROUP BY p.id, p.nombre, p.categoria_id, c.nombre, p.markup_pct_override,
                 p.iva_pct_override, c.markup_pct, c.iva_pct, p.unidad_de_venta,
                 p.controla_vencimiento, p.precio_actual_centavos, p.costo_actual_centavos, p.activo
        ORDER BY CASE WHEN $1::text IS NULL THEN 0 ELSE similarity(p.nombre, $1) END DESC, p.nombre
        LIMIT $3
        "#,
        buscar,
        incluir_inactivos,
        limite,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        filas
            .into_iter()
            .map(|f| ProductoDetalle {
                id: f.id,
                nombre: f.nombre,
                categoria_id: f.categoria_id,
                categoria_nombre: f.categoria_nombre,
                markup_pct_override: f.markup_pct_override,
                iva_pct_override: f.iva_pct_override,
                markup_pct_resuelto: f.markup_pct_resuelto,
                iva_pct_resuelto: f.iva_pct_resuelto,
                unidad_de_venta: f.unidad_de_venta,
                controla_vencimiento: f.controla_vencimiento,
                precio_actual_centavos: f.precio_actual_centavos,
                costo_actual_centavos: f.costo_actual_centavos,
                activo: f.activo,
                codigos_barras: f.codigos_barras,
            })
            .collect(),
    ))
}

async fn cargar_producto_detalle(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<ProductoDetalle, ErrorApi> {
    let f = sqlx::query!(
        r#"
        SELECT p.id, p.nombre, p.categoria_id, c.nombre AS categoria_nombre,
               p.markup_pct_override, p.iva_pct_override,
               COALESCE(p.markup_pct_override, c.markup_pct) AS "markup_pct_resuelto!",
               COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct_resuelto!",
               p.unidad_de_venta AS "unidad_de_venta: UnidadDeVenta",
               p.controla_vencimiento, p.precio_actual_centavos, p.costo_actual_centavos,
               p.activo,
               COALESCE(array_agg(cb.codigo) FILTER (WHERE cb.codigo IS NOT NULL), '{}') AS "codigos_barras!"
        FROM catalogo.productos p
        JOIN catalogo.categorias c ON c.id = p.categoria_id
        LEFT JOIN catalogo.codigos_barras cb ON cb.producto_id = p.id
        WHERE p.id = $1
        GROUP BY p.id, p.nombre, p.categoria_id, c.nombre, p.markup_pct_override,
                 p.iva_pct_override, c.markup_pct, c.iva_pct, p.unidad_de_venta,
                 p.controla_vencimiento, p.precio_actual_centavos, p.costo_actual_centavos, p.activo
        "#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    Ok(ProductoDetalle {
        id: f.id,
        nombre: f.nombre,
        categoria_id: f.categoria_id,
        categoria_nombre: f.categoria_nombre,
        markup_pct_override: f.markup_pct_override,
        iva_pct_override: f.iva_pct_override,
        markup_pct_resuelto: f.markup_pct_resuelto,
        iva_pct_resuelto: f.iva_pct_resuelto,
        unidad_de_venta: f.unidad_de_venta,
        controla_vencimiento: f.controla_vencimiento,
        precio_actual_centavos: f.precio_actual_centavos,
        costo_actual_centavos: f.costo_actual_centavos,
        activo: f.activo,
        codigos_barras: f.codigos_barras,
    })
}

async fn obtener_producto(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductoDetalle>, ErrorApi> {
    Ok(Json(cargar_producto_detalle(&estado.pool, id).await?))
}

#[derive(Deserialize)]
struct CrearProducto {
    nombre: String,
    categoria_id: Uuid,
    markup_pct_override: Option<Decimal>,
    iva_pct_override: Option<Decimal>,
    unidad_de_venta: Option<UnidadDeVenta>,
    controla_vencimiento: Option<bool>,
    #[serde(default)]
    codigos_barras: Vec<String>,
}

async fn crear_producto(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Json(datos): Json<CrearProducto>,
) -> Result<Json<ProductoDetalle>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }
    if let Some(v) = datos.markup_pct_override {
        validar_pct("markup_pct_override", v)?;
    }
    if let Some(v) = datos.iva_pct_override {
        validar_pct("iva_pct_override", v)?;
    }

    let id = Uuid::now_v7();
    let unidad = datos.unidad_de_venta.unwrap_or(UnidadDeVenta::Unidad);
    let mut tx = estado.pool.begin().await?;

    sqlx::query!(
        r#"SELECT id FROM catalogo.categorias WHERE id = $1 AND activo"#,
        datos.categoria_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ErrorApi::Validacion("categoría inexistente o inactiva".into()))?;

    sqlx::query!(
        r#"
        INSERT INTO catalogo.productos
            (id, nombre, categoria_id, markup_pct_override, iva_pct_override,
             unidad_de_venta, controla_vencimiento)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        id,
        datos.nombre.trim(),
        datos.categoria_id,
        datos.markup_pct_override,
        datos.iva_pct_override,
        unidad as UnidadDeVenta,
        datos.controla_vencimiento.unwrap_or(false),
    )
    .execute(&mut *tx)
    .await?;

    for codigo in &datos.codigos_barras {
        let codigo = codigo.trim();
        if codigo.is_empty() {
            continue;
        }
        let insertado = sqlx::query!(
            r#"INSERT INTO catalogo.codigos_barras (codigo, producto_id) VALUES ($1, $2)
               ON CONFLICT (codigo) DO NOTHING RETURNING codigo"#,
            codigo,
            id,
        )
        .fetch_optional(&mut *tx)
        .await?;
        if insertado.is_none() {
            return Err(ErrorApi::Conflicto(format!(
                "el código de barras {codigo} ya está asignado a otro producto"
            )));
        }
    }

    auditoria::registrar(
        &mut *tx,
        "producto",
        Some(id),
        "crear",
        Some(usuario.id),
        Some(json!({
            "nombre": datos.nombre.trim(),
            "categoria_id": datos.categoria_id,
            "codigos_barras": datos.codigos_barras,
        })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(cargar_producto_detalle(&estado.pool, id).await?))
}

#[derive(Deserialize)]
struct ActualizarProducto {
    nombre: Option<String>,
    categoria_id: Option<Uuid>,
    #[serde(default)]
    markup_pct_override: Option<Option<Decimal>>,
    #[serde(default)]
    iva_pct_override: Option<Option<Decimal>>,
    unidad_de_venta: Option<UnidadDeVenta>,
    controla_vencimiento: Option<bool>,
    activo: Option<bool>,
}

async fn actualizar_producto(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarProducto>,
) -> Result<Json<ProductoDetalle>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    if let Some(Some(v)) = datos.markup_pct_override {
        validar_pct("markup_pct_override", v)?;
    }
    if let Some(Some(v)) = datos.iva_pct_override {
        validar_pct("iva_pct_override", v)?;
    }

    let mut tx = estado.pool.begin().await?;
    let antes = sqlx::query!(
        r#"
        SELECT nombre, categoria_id, markup_pct_override, iva_pct_override,
               controla_vencimiento, activo
        FROM catalogo.productos WHERE id = $1 FOR UPDATE
        "#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    if let Some(categoria_id) = datos.categoria_id {
        sqlx::query!(r#"SELECT id FROM catalogo.categorias WHERE id = $1 AND activo"#, categoria_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("categoría inexistente o inactiva".into()))?;
    }

    let (cambiar_markup, nuevo_markup) = match datos.markup_pct_override {
        Some(v) => (true, v),
        None => (false, None),
    };
    let (cambiar_iva, nuevo_iva) = match datos.iva_pct_override {
        Some(v) => (true, v),
        None => (false, None),
    };

    sqlx::query!(
        r#"
        UPDATE catalogo.productos SET
            nombre = COALESCE($2, nombre),
            categoria_id = COALESCE($3, categoria_id),
            markup_pct_override = CASE WHEN $4 THEN $5 ELSE markup_pct_override END,
            iva_pct_override = CASE WHEN $6 THEN $7 ELSE iva_pct_override END,
            unidad_de_venta = COALESCE($8, unidad_de_venta),
            controla_vencimiento = COALESCE($9, controla_vencimiento),
            activo = COALESCE($10, activo),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        datos.categoria_id,
        cambiar_markup,
        nuevo_markup,
        cambiar_iva,
        nuevo_iva,
        datos.unidad_de_venta as Option<UnidadDeVenta>,
        datos.controla_vencimiento,
        datos.activo,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "producto",
        Some(id),
        "actualizar",
        Some(usuario.id),
        Some(auditoria::diff_antes_despues(
            json!({
                "nombre": antes.nombre, "categoria_id": antes.categoria_id,
                "markup_pct_override": antes.markup_pct_override,
                "iva_pct_override": antes.iva_pct_override,
                "controla_vencimiento": antes.controla_vencimiento,
                "activo": antes.activo,
            }),
            json!({
                "nombre": datos.nombre, "categoria_id": datos.categoria_id,
                "markup_pct_override": datos.markup_pct_override,
                "iva_pct_override": datos.iva_pct_override,
                "controla_vencimiento": datos.controla_vencimiento,
                "activo": datos.activo,
            }),
        )),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(cargar_producto_detalle(&estado.pool, id).await?))
}

async fn desactivar_producto(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    let mut tx = estado.pool.begin().await?;
    let resultado = sqlx::query!(
        r#"UPDATE catalogo.productos SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;
    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "producto", Some(id), "desactivar", Some(usuario.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Códigos de barras ----------

#[derive(Deserialize)]
struct NuevoCodigoBarras {
    codigo: String,
    descripcion: Option<String>,
}

async fn agregar_codigo_barras(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<NuevoCodigoBarras>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    let codigo = datos.codigo.trim();
    if codigo.is_empty() {
        return Err(ErrorApi::Validacion("el código es obligatorio".into()));
    }

    let mut tx = estado.pool.begin().await?;
    sqlx::query!(r#"SELECT id FROM catalogo.productos WHERE id = $1"#, id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ErrorApi::NoEncontrado)?;

    let existente = sqlx::query!(
        r#"SELECT producto_id FROM catalogo.codigos_barras WHERE codigo = $1"#,
        codigo,
    )
    .fetch_optional(&mut *tx)
    .await?;
    match existente {
        // Idempotente: mismo código para el mismo producto = no-op.
        Some(e) if e.producto_id == id => return Ok(Json(json!({ "ok": true }))),
        Some(_) => {
            return Err(ErrorApi::Conflicto(
                "el código ya está asignado a otro producto".into(),
            ))
        }
        None => {}
    }

    sqlx::query!(
        r#"INSERT INTO catalogo.codigos_barras (codigo, producto_id, descripcion)
           VALUES ($1, $2, $3)"#,
        codigo,
        id,
        datos.descripcion,
    )
    .execute(&mut *tx)
    .await?;

    auditoria::registrar(
        &mut *tx,
        "producto",
        Some(id),
        "agregar_codigo_barras",
        Some(usuario.id),
        Some(json!({ "codigo": codigo })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn quitar_codigo_barras(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(codigo): Path<String>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_CATALOGO)?;
    let mut tx = estado.pool.begin().await?;
    let fila = sqlx::query!(
        r#"DELETE FROM catalogo.codigos_barras WHERE codigo = $1 RETURNING producto_id"#,
        codigo,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    auditoria::registrar(
        &mut *tx,
        "producto",
        Some(fila.producto_id),
        "quitar_codigo_barras",
        Some(usuario.id),
        Some(json!({ "codigo": codigo })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// Hot path del escaneo: código → producto con precio e IVA resueltos.
async fn resolver_codigo_barras(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(codigo): Path<String>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    let f = sqlx::query!(
        r#"
        SELECT p.id, p.nombre,
               p.unidad_de_venta AS "unidad_de_venta: UnidadDeVenta",
               p.precio_actual_centavos,
               COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct!",
               p.activo
        FROM catalogo.codigos_barras cb
        JOIN catalogo.productos p ON p.id = cb.producto_id
        JOIN catalogo.categorias c ON c.id = p.categoria_id
        WHERE cb.codigo = $1
        "#,
        codigo,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    Ok(Json(json!({
        "producto_id": f.id,
        "nombre": f.nombre,
        "unidad_de_venta": f.unidad_de_venta,
        "precio_actual_centavos": f.precio_actual_centavos,
        "iva_pct": f.iva_pct,
        "activo": f.activo,
    })))
}

// ---------- Precios ----------

#[derive(Deserialize)]
struct CambioPrecioManual {
    precio_centavos: i64,
}

async fn cambiar_precio_manual(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<CambioPrecioManual>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    usuario.exigir(permisos::MODIFICAR_PRECIOS)?;
    if datos.precio_centavos < 0 {
        return Err(ErrorApi::Validacion("el precio no puede ser negativo".into()));
    }

    let mut tx = estado.pool.begin().await?;
    let producto = sqlx::query!(
        r#"SELECT costo_actual_centavos FROM catalogo.productos WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    // Ledger + proyección en la misma transacción. recepcion_id NULL = manual.
    sqlx::query!(
        r#"
        INSERT INTO catalogo.precios_historial
            (id, producto_id, precio_centavos, costo_centavos, recepcion_id, usuario_id)
        VALUES ($1, $2, $3, $4, NULL, $5)
        "#,
        Uuid::now_v7(),
        id,
        datos.precio_centavos,
        producto.costo_actual_centavos,
        usuario.id,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"UPDATE catalogo.productos SET precio_actual_centavos = $2, actualizado_en = now()
           WHERE id = $1"#,
        id,
        datos.precio_centavos,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "precio_centavos": datos.precio_centavos })))
}

#[derive(Serialize)]
struct EntradaHistorialPrecio {
    id: Uuid,
    precio_centavos: i64,
    costo_centavos: Option<i64>,
    recepcion_id: Option<Uuid>,
    usuario_id: Uuid,
    vigente_desde: DateTime<Utc>,
}

async fn historial_precios(
    State(estado): State<Estado>,
    _usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<EntradaHistorialPrecio>>, ErrorApi> {
    let entradas = sqlx::query_as!(
        EntradaHistorialPrecio,
        r#"
        SELECT id, precio_centavos, costo_centavos, recepcion_id, usuario_id, vigente_desde
        FROM catalogo.precios_historial
        WHERE producto_id = $1
        ORDER BY vigente_desde DESC
        LIMIT 200
        "#,
        id,
    )
    .fetch_all(&estado.pool)
    .await?;
    Ok(Json(entradas))
}
