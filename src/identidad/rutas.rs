use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auditoria;
use crate::error::ErrorApi;
use crate::estado::Estado;
use crate::identidad::auth::{
    self, cargar_usuario_con_permisos, emitir_token, hashear_secreto, verificar_secreto,
    UsuarioActual,
};
use crate::identidad::permisos;

pub fn router() -> Router<Estado> {
    Router::new()
        .route("/login", post(login))
        .route("/login-pin", post(login_pin))
        .route("/yo", get(yo))
        .route("/permisos", get(listar_permisos))
        .route("/usuarios", get(listar_usuarios).post(crear_usuario))
        .route(
            "/usuarios/{id}",
            get(obtener_usuario)
                .patch(actualizar_usuario)
                .delete(desactivar_usuario),
        )
        .route("/roles", get(listar_roles).post(crear_rol))
        .route("/roles/{id}", axum::routing::patch(actualizar_rol).delete(desactivar_rol))
}

// ---------- Autenticación ----------

#[derive(Deserialize)]
struct CredencialesPassword {
    nombre: String,
    password: String,
}

#[derive(Deserialize)]
struct CredencialesPin {
    nombre: String,
    pin: String,
}

#[derive(Serialize)]
struct RespuestaLogin {
    token: String,
    usuario: PerfilUsuario,
}

#[derive(Serialize)]
struct PerfilUsuario {
    id: Uuid,
    nombre: String,
    permisos: Vec<String>,
}

async fn login(
    State(estado): State<Estado>,
    Json(cred): Json<CredencialesPassword>,
) -> Result<Json<RespuestaLogin>, ErrorApi> {
    let fila = sqlx::query!(
        r#"SELECT id, password_hash, activo FROM identidad.usuarios WHERE nombre = $1"#,
        cred.nombre,
    )
    .fetch_optional(&estado.pool)
    .await?;

    let valido = fila
        .as_ref()
        .map(|f| f.activo && verificar_secreto(&f.password_hash, &cred.password))
        .unwrap_or(false);

    if !valido {
        // Login administrativo fallido: acción de seguridad, se audita.
        auditoria::registrar(
            &estado.pool,
            "usuario",
            fila.as_ref().map(|f| f.id),
            "login_fallido",
            None,
            Some(json!({ "nombre_intentado": cred.nombre })),
        )
        .await?;
        return Err(ErrorApi::NoAutenticado);
    }

    responder_login(&estado, fila.unwrap().id).await
}

async fn login_pin(
    State(estado): State<Estado>,
    Json(cred): Json<CredencialesPin>,
) -> Result<Json<RespuestaLogin>, ErrorApi> {
    let fila = sqlx::query!(
        r#"SELECT id, pin_hash, activo FROM identidad.usuarios WHERE nombre = $1"#,
        cred.nombre,
    )
    .fetch_optional(&estado.pool)
    .await?;

    let valido = fila
        .as_ref()
        .map(|f| {
            f.activo
                && f.pin_hash
                    .as_deref()
                    .map(|h| verificar_secreto(h, &cred.pin))
                    .unwrap_or(false)
        })
        .unwrap_or(false);

    if !valido {
        return Err(ErrorApi::NoAutenticado);
    }

    responder_login(&estado, fila.unwrap().id).await
}

async fn responder_login(estado: &Estado, usuario_id: Uuid) -> Result<Json<RespuestaLogin>, ErrorApi> {
    let usuario = cargar_usuario_con_permisos(&estado.pool, usuario_id).await?;
    let token = emitir_token(usuario.id, &estado.jwt_secret)?;
    Ok(Json(RespuestaLogin {
        token,
        usuario: PerfilUsuario {
            id: usuario.id,
            nombre: usuario.nombre,
            permisos: usuario.permisos.into_iter().collect(),
        },
    }))
}

async fn yo(usuario: UsuarioActual) -> Json<PerfilUsuario> {
    Json(PerfilUsuario {
        id: usuario.id,
        nombre: usuario.nombre,
        permisos: usuario.permisos.into_iter().collect(),
    })
}

async fn listar_permisos(usuario: UsuarioActual) -> Result<Json<Vec<&'static str>>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_USUARIOS)?;
    Ok(Json(permisos::TODOS.to_vec()))
}

// ---------- Usuarios ----------

#[derive(Serialize)]
struct UsuarioResumen {
    id: Uuid,
    nombre: String,
    rol_id: Uuid,
    rol_nombre: String,
    tiene_pin: bool,
    permisos_extra: Vec<String>,
    activo: bool,
}

#[derive(Deserialize)]
struct FiltroInactivos {
    incluir_inactivos: Option<bool>,
}

async fn listar_usuarios(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Query(filtro): Query<FiltroInactivos>,
) -> Result<Json<Vec<UsuarioResumen>>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_USUARIOS)?;
    let incluir_inactivos = filtro.incluir_inactivos.unwrap_or(false);
    let usuarios = sqlx::query!(
        r#"
        SELECT u.id, u.nombre, u.rol_id, r.nombre AS rol_nombre,
               u.pin_hash IS NOT NULL AS "tiene_pin!",
               COALESCE(array_agg(up.permiso) FILTER (WHERE up.permiso IS NOT NULL), '{}') AS "permisos_extra!",
               u.activo
        FROM identidad.usuarios u
        JOIN identidad.roles r ON r.id = u.rol_id
        LEFT JOIN identidad.usuario_permisos up ON up.usuario_id = u.id
        WHERE u.activo OR $1
        GROUP BY u.id, u.nombre, u.rol_id, r.nombre, u.pin_hash, u.activo
        ORDER BY u.nombre
        "#,
        incluir_inactivos,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        usuarios
            .into_iter()
            .map(|u| UsuarioResumen {
                id: u.id,
                nombre: u.nombre,
                rol_id: u.rol_id,
                rol_nombre: u.rol_nombre,
                tiene_pin: u.tiene_pin,
                permisos_extra: u.permisos_extra,
                activo: u.activo,
            })
            .collect(),
    ))
}

async fn obtener_usuario(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<UsuarioResumen>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_USUARIOS)?;
    let u = sqlx::query!(
        r#"
        SELECT u.id, u.nombre, u.rol_id, r.nombre AS rol_nombre,
               u.pin_hash IS NOT NULL AS "tiene_pin!",
               COALESCE(array_agg(up.permiso) FILTER (WHERE up.permiso IS NOT NULL), '{}') AS "permisos_extra!",
               u.activo
        FROM identidad.usuarios u
        JOIN identidad.roles r ON r.id = u.rol_id
        LEFT JOIN identidad.usuario_permisos up ON up.usuario_id = u.id
        WHERE u.id = $1
        GROUP BY u.id, u.nombre, u.rol_id, r.nombre, u.pin_hash, u.activo
        "#,
        id,
    )
    .fetch_optional(&estado.pool)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    Ok(Json(UsuarioResumen {
        id: u.id,
        nombre: u.nombre,
        rol_id: u.rol_id,
        rol_nombre: u.rol_nombre,
        tiene_pin: u.tiene_pin,
        permisos_extra: u.permisos_extra,
        activo: u.activo,
    }))
}

#[derive(Deserialize)]
struct CrearUsuario {
    nombre: String,
    password: String,
    pin: Option<String>,
    rol_id: Uuid,
    #[serde(default)]
    permisos_extra: Vec<String>,
}

fn validar_permisos(lista: &[String]) -> Result<(), ErrorApi> {
    for p in lista {
        if !permisos::es_valido(p) {
            return Err(ErrorApi::Validacion(format!("permiso desconocido: {p}")));
        }
    }
    Ok(())
}

async fn crear_usuario(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Json(datos): Json<CrearUsuario>,
) -> Result<Json<UsuarioResumen>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }
    if datos.password.len() < 8 {
        return Err(ErrorApi::Validacion(
            "la contraseña debe tener al menos 8 caracteres".into(),
        ));
    }
    validar_permisos(&datos.permisos_extra)?;
    if let Some(pin) = &datos.pin {
        auth::validar_formato_pin(pin)?;
    }

    let password_hash = hashear_secreto(&datos.password)?;
    let pin_hash = datos.pin.as_deref().map(hashear_secreto).transpose()?;
    let id = Uuid::now_v7();

    let mut tx = estado.pool.begin().await?;

    let rol = sqlx::query!(
        r#"SELECT nombre FROM identidad.roles WHERE id = $1 AND activo"#,
        datos.rol_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ErrorApi::Validacion("rol inexistente o inactivo".into()))?;

    let insertado = sqlx::query!(
        r#"
        INSERT INTO identidad.usuarios (id, nombre, password_hash, pin_hash, rol_id)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (nombre) DO NOTHING
        RETURNING id
        "#,
        id,
        datos.nombre.trim(),
        password_hash,
        pin_hash,
        datos.rol_id,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if insertado.is_none() {
        return Err(ErrorApi::Conflicto("ya existe un usuario con ese nombre".into()));
    }

    for p in &datos.permisos_extra {
        sqlx::query!(
            r#"INSERT INTO identidad.usuario_permisos (usuario_id, permiso) VALUES ($1, $2)
               ON CONFLICT DO NOTHING"#,
            id,
            p,
        )
        .execute(&mut *tx)
        .await?;
    }

    auditoria::registrar(
        &mut *tx,
        "usuario",
        Some(id),
        "crear",
        Some(ejecutor.id),
        Some(json!({
            "nombre": datos.nombre.trim(),
            "rol_id": datos.rol_id,
            "permisos_extra": datos.permisos_extra,
            "tiene_pin": datos.pin.is_some(),
        })),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(UsuarioResumen {
        id,
        nombre: datos.nombre.trim().to_string(),
        rol_id: datos.rol_id,
        rol_nombre: rol.nombre,
        tiene_pin: datos.pin.is_some(),
        permisos_extra: datos.permisos_extra,
        activo: true,
    }))
}

#[derive(Deserialize)]
struct ActualizarUsuario {
    nombre: Option<String>,
    rol_id: Option<Uuid>,
    password: Option<String>,
    pin: Option<String>,
    permisos_extra: Option<Vec<String>>,
    activo: Option<bool>,
}

async fn actualizar_usuario(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarUsuario>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;

    if let Some(permisos_extra) = &datos.permisos_extra {
        validar_permisos(permisos_extra)?;
    }
    if let Some(pin) = &datos.pin {
        auth::validar_formato_pin(pin)?;
    }
    if let Some(password) = &datos.password {
        if password.len() < 8 {
            return Err(ErrorApi::Validacion(
                "la contraseña debe tener al menos 8 caracteres".into(),
            ));
        }
    }

    let mut tx = estado.pool.begin().await?;

    let antes = sqlx::query!(
        r#"SELECT nombre, rol_id, activo FROM identidad.usuarios WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    if let Some(rol_id) = datos.rol_id {
        sqlx::query!(r#"SELECT id FROM identidad.roles WHERE id = $1 AND activo"#, rol_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ErrorApi::Validacion("rol inexistente o inactivo".into()))?;
    }

    let password_hash = datos.password.as_deref().map(hashear_secreto).transpose()?;
    let pin_hash = datos.pin.as_deref().map(hashear_secreto).transpose()?;

    sqlx::query!(
        r#"
        UPDATE identidad.usuarios SET
            nombre = COALESCE($2, nombre),
            rol_id = COALESCE($3, rol_id),
            password_hash = COALESCE($4, password_hash),
            pin_hash = COALESCE($5, pin_hash),
            activo = COALESCE($6, activo),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        datos.rol_id,
        password_hash,
        pin_hash,
        datos.activo,
    )
    .execute(&mut *tx)
    .await?;

    if let Some(permisos_extra) = &datos.permisos_extra {
        sqlx::query!(r#"DELETE FROM identidad.usuario_permisos WHERE usuario_id = $1"#, id)
            .execute(&mut *tx)
            .await?;
        for p in permisos_extra {
            sqlx::query!(
                r#"INSERT INTO identidad.usuario_permisos (usuario_id, permiso) VALUES ($1, $2)"#,
                id,
                p,
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    auditoria::registrar(
        &mut *tx,
        "usuario",
        Some(id),
        "actualizar",
        Some(ejecutor.id),
        Some(auditoria::diff_antes_despues(
            json!({ "nombre": antes.nombre, "rol_id": antes.rol_id, "activo": antes.activo }),
            json!({
                "nombre": datos.nombre,
                "rol_id": datos.rol_id,
                "activo": datos.activo,
                "permisos_extra": datos.permisos_extra,
                "password_cambiada": datos.password.is_some(),
                "pin_cambiado": datos.pin.is_some(),
            }),
        )),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn desactivar_usuario(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;
    if ejecutor.id == id {
        return Err(ErrorApi::Validacion("no podés desactivarte a vos mismo".into()));
    }

    let mut tx = estado.pool.begin().await?;
    let resultado = sqlx::query!(
        r#"UPDATE identidad.usuarios SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;

    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "usuario", Some(id), "desactivar", Some(ejecutor.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Roles ----------

#[derive(Serialize)]
struct RolConPermisos {
    id: Uuid,
    nombre: String,
    descripcion: Option<String>,
    permisos: Vec<String>,
    activo: bool,
}

async fn listar_roles(
    State(estado): State<Estado>,
    usuario: UsuarioActual,
) -> Result<Json<Vec<RolConPermisos>>, ErrorApi> {
    usuario.exigir(permisos::GESTIONAR_USUARIOS)?;
    let roles = sqlx::query!(
        r#"
        SELECT r.id, r.nombre, r.descripcion,
               COALESCE(array_agg(rp.permiso ORDER BY rp.permiso)
                        FILTER (WHERE rp.permiso IS NOT NULL), '{}') AS "permisos!",
               r.activo
        FROM identidad.roles r
        LEFT JOIN identidad.rol_permisos rp ON rp.rol_id = r.id
        GROUP BY r.id, r.nombre, r.descripcion, r.activo
        ORDER BY r.nombre
        "#,
    )
    .fetch_all(&estado.pool)
    .await?;

    Ok(Json(
        roles
            .into_iter()
            .map(|r| RolConPermisos {
                id: r.id,
                nombre: r.nombre,
                descripcion: r.descripcion,
                permisos: r.permisos,
                activo: r.activo,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct CrearRol {
    nombre: String,
    descripcion: Option<String>,
    permisos: Vec<String>,
}

async fn crear_rol(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Json(datos): Json<CrearRol>,
) -> Result<Json<RolConPermisos>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;
    if datos.nombre.trim().is_empty() {
        return Err(ErrorApi::Validacion("el nombre es obligatorio".into()));
    }
    validar_permisos(&datos.permisos)?;

    let id = Uuid::now_v7();
    let mut tx = estado.pool.begin().await?;

    let insertado = sqlx::query!(
        r#"
        INSERT INTO identidad.roles (id, nombre, descripcion)
        VALUES ($1, $2, $3)
        ON CONFLICT (nombre) DO NOTHING
        RETURNING id
        "#,
        id,
        datos.nombre.trim(),
        datos.descripcion,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if insertado.is_none() {
        return Err(ErrorApi::Conflicto("ya existe un rol con ese nombre".into()));
    }

    for p in &datos.permisos {
        sqlx::query!(
            r#"INSERT INTO identidad.rol_permisos (rol_id, permiso) VALUES ($1, $2)
               ON CONFLICT DO NOTHING"#,
            id,
            p,
        )
        .execute(&mut *tx)
        .await?;
    }

    auditoria::registrar(
        &mut *tx,
        "rol",
        Some(id),
        "crear",
        Some(ejecutor.id),
        Some(json!({ "nombre": datos.nombre.trim(), "permisos": datos.permisos })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(RolConPermisos {
        id,
        nombre: datos.nombre.trim().to_string(),
        descripcion: datos.descripcion,
        permisos: datos.permisos,
        activo: true,
    }))
}

#[derive(Deserialize)]
struct ActualizarRol {
    nombre: Option<String>,
    descripcion: Option<String>,
    permisos: Option<Vec<String>>,
}

async fn actualizar_rol(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Path(id): Path<Uuid>,
    Json(datos): Json<ActualizarRol>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;
    if let Some(lista) = &datos.permisos {
        validar_permisos(lista)?;
    }

    let mut tx = estado.pool.begin().await?;

    let antes = sqlx::query!(
        r#"
        SELECT r.nombre, r.descripcion,
               COALESCE(array_agg(rp.permiso) FILTER (WHERE rp.permiso IS NOT NULL), '{}') AS "permisos!"
        FROM identidad.roles r
        LEFT JOIN identidad.rol_permisos rp ON rp.rol_id = r.id
        WHERE r.id = $1
        GROUP BY r.id, r.nombre, r.descripcion
        "#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ErrorApi::NoEncontrado)?;

    sqlx::query!(
        r#"
        UPDATE identidad.roles SET
            nombre = COALESCE($2, nombre),
            descripcion = COALESCE($3, descripcion),
            actualizado_en = now()
        WHERE id = $1
        "#,
        id,
        datos.nombre.as_deref().map(str::trim),
        datos.descripcion,
    )
    .execute(&mut *tx)
    .await?;

    if let Some(lista) = &datos.permisos {
        sqlx::query!(r#"DELETE FROM identidad.rol_permisos WHERE rol_id = $1"#, id)
            .execute(&mut *tx)
            .await?;
        for p in lista {
            sqlx::query!(
                r#"INSERT INTO identidad.rol_permisos (rol_id, permiso) VALUES ($1, $2)"#,
                id,
                p,
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    auditoria::registrar(
        &mut *tx,
        "rol",
        Some(id),
        "actualizar",
        Some(ejecutor.id),
        Some(auditoria::diff_antes_despues(
            json!({ "nombre": antes.nombre, "descripcion": antes.descripcion, "permisos": antes.permisos }),
            json!({ "nombre": datos.nombre, "descripcion": datos.descripcion, "permisos": datos.permisos }),
        )),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn desactivar_rol(
    State(estado): State<Estado>,
    ejecutor: UsuarioActual,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ErrorApi> {
    ejecutor.exigir(permisos::GESTIONAR_USUARIOS)?;

    let mut tx = estado.pool.begin().await?;

    let en_uso = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM identidad.usuarios WHERE rol_id = $1 AND activo) AS "en_uso!""#,
        id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if en_uso {
        return Err(ErrorApi::Conflicto(
            "hay usuarios activos con este rol; reasignalos antes de desactivarlo".into(),
        ));
    }

    let resultado = sqlx::query!(
        r#"UPDATE identidad.roles SET activo = false, actualizado_en = now()
           WHERE id = $1 AND activo"#,
        id,
    )
    .execute(&mut *tx)
    .await?;

    if resultado.rows_affected() > 0 {
        auditoria::registrar(&mut *tx, "rol", Some(id), "desactivar", Some(ejecutor.id), None)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
