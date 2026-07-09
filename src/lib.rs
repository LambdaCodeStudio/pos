pub mod auditoria;
pub mod catalogo;
pub mod clientes;
pub mod compras;
pub mod error;
pub mod estado;
pub mod identidad;
pub mod inventario;
pub mod reportes;
pub mod ventas;

use axum::routing::get;
use axum::Router;
use estado::Estado;

pub fn armar_router(estado: Estado) -> Router {
    Router::new()
        .route("/salud", get(|| async { "ok" }))
        .nest("/identidad", identidad::rutas::router())
        .nest("/catalogo", catalogo::rutas::router())
        .nest("/compras", compras::rutas::router())
        .nest("/inventario", inventario::rutas::router())
        .nest("/ventas", ventas::rutas::router())
        .nest("/clientes", clientes::rutas::router())
        .nest("/reportes", reportes::rutas::router())
        .nest("/auditoria", auditoria::rutas::router())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(estado)
}
