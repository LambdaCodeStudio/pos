//! Verificación de permisos: denegado por defecto, rol + permisos
//! individuales aditivos, sin deny-overrides.

mod comun;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use comun::{app, crear_usuario_con_rol, pedir, token_para, ROL_ADMINISTRADOR_ID, ROL_CAJERO_ID};

#[sqlx::test(migrations = "./migrations")]
async fn sin_token_es_401(pool: PgPool) {
    let app = app(pool);
    let (st, _) = pedir(&app, "GET", "/catalogo/productos", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn cajero_no_puede_confirmar_recepciones(pool: PgPool) {
    let cajero = crear_usuario_con_rol(&pool, "cajero", ROL_CAJERO_ID).await;
    let token = token_para(cajero);
    let app = app(pool);

    // Puede leer el catálogo (lectura autenticada sin permiso especial).
    let (st, _) = pedir(&app, "GET", "/catalogo/productos", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);

    // No puede crear recepciones: denegado por defecto.
    let (st, _) = pedir(&app, "POST", "/compras/recepciones", Some(&token), Some(json!({}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Tampoco gestionar usuarios.
    let (st, _) = pedir(&app, "GET", "/identidad/usuarios", Some(&token), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn permiso_individual_es_aditivo(pool: PgPool) {
    let cajero = crear_usuario_con_rol(&pool, "cajero-plus", ROL_CAJERO_ID).await;
    sqlx::query(
        "INSERT INTO identidad.usuario_permisos (usuario_id, permiso) VALUES ($1, 'confirmar_recepcion')",
    )
    .bind(cajero)
    .execute(&pool)
    .await
    .unwrap();

    let token = token_para(cajero);
    let app = app(pool);

    let (st, resp) =
        pedir(&app, "POST", "/compras/recepciones", Some(&token), Some(json!({}))).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
}

#[sqlx::test(migrations = "./migrations")]
async fn usuario_desactivado_pierde_acceso_inmediato(pool: PgPool) {
    let admin = crear_usuario_con_rol(&pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (st, _) = pedir(&app, "GET", "/identidad/usuarios", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);

    // Desactivado en la base: el token vigente deja de servir en el acto
    // (los permisos se resuelven por request, no viajan en el JWT).
    sqlx::query("UPDATE identidad.usuarios SET activo = false WHERE id = $1")
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();

    let (st, _) = pedir(&app, "GET", "/identidad/usuarios", Some(&token), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn eventos_de_auditoria_llegan_enriquecidos(pool: PgPool) {
    let admin = crear_usuario_con_rol(&pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    // Una mutación de maestro genera su evento.
    let (st, cat) = pedir(&app, "POST", "/catalogo/categorias", Some(&token),
        Some(json!({ "nombre": "Bebidas" }))).await;
    assert_eq!(st, StatusCode::OK);

    let (st, r) = pedir(&app, "GET", "/auditoria/eventos?entidad=categoria", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let eventos = r["eventos"].as_array().unwrap();
    assert_eq!(eventos.len(), 1);
    assert_eq!(eventos[0]["accion"], "crear");
    assert_eq!(eventos[0]["entidad_nombre"], "Bebidas", "resuelve el nombre de la entidad: {r}");
    assert_eq!(eventos[0]["usuario_nombre"], "admin-test", "resuelve quién lo hizo");
    assert_eq!(eventos[0]["entidad_id"], cat["id"]);

    // El filtro por acción también funciona.
    let (_, sin_resultados) = pedir(&app, "GET", "/auditoria/eventos?entidad=categoria&accion=desactivar",
        Some(&token), None).await;
    assert_eq!(sin_resultados["eventos"].as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn login_fallido_queda_auditado(pool: PgPool) {
    let app = app(pool.clone());
    let (st, _) = pedir(
        &app,
        "POST",
        "/identidad/login",
        None,
        Some(json!({ "nombre": "no-existe", "password": "cualquiera" })),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let eventos = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM auditoria.auditoria_eventos WHERE accion = 'login_fallido'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(eventos, 1);
}
