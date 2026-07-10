//! Tests de los endpoints de métricas: agregaciones correctas, ventas
//! anuladas excluidas de la facturación, y permiso ver_reportes exigido.

mod comun;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use comun::{app, crear_usuario_con_rol, pedir, token_para, ROL_ADMINISTRADOR_ID, ROL_CAJERO_ID};

#[sqlx::test(migrations = "./migrations")]
async fn resumen_de_ventas_agrega_y_excluye_anuladas(pool: PgPool) {
    let admin = crear_usuario_con_rol(&pool, "admin-test", ROL_ADMINISTRADOR_ID).await;
    let token = token_para(admin);
    let app = app(pool.clone());

    let (_, cat) = pedir(&app, "POST", "/catalogo/categorias", Some(&token),
        Some(json!({ "nombre": "Almacén" }))).await;
    let (_, prod) = pedir(&app, "POST", "/catalogo/productos", Some(&token),
        Some(json!({ "nombre": "Fideos 500g", "categoria_id": cat["id"] }))).await;
    let producto = prod["id"].as_str().unwrap();
    let (_, sesion) = pedir(&app, "POST", "/ventas/sesiones", Some(&token),
        Some(json!({ "monto_inicial_centavos": 0 }))).await;
    let sesion_id = sesion["id"].as_str().unwrap();

    let vender = |total: i64, medio: &'static str| {
        let id = Uuid::now_v7();
        (id, json!({
            "id": id,
            "sesion_id": sesion_id,
            "total_centavos": total,
            "vendida_en": chrono::Utc::now(),
            "items": [{
                "producto_id": producto,
                "producto_nombre": "Fideos 500g",
                "precio_unitario_centavos": total,
                "cantidad": "1",
                "subtotal_centavos": total,
            }],
            "pagos": [{ "medio": medio, "monto_centavos": total }],
        }))
    };

    // Dos ventas confirmadas (efectivo y tarjeta) y una anulada.
    let (_, v1) = vender(1000, "efectivo");
    let (_, v2) = vender(2500, "tarjeta");
    let (id3, v3) = vender(9000, "efectivo");
    for v in [&v1, &v2, &v3] {
        let (st, r) = pedir(&app, "POST", "/ventas", Some(&token), Some(v.clone())).await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    let (st, _) = pedir(&app, "POST", &format!("/ventas/{id3}/anular"), Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);

    let (st, resumen) = pedir(&app, "GET", "/reportes/ventas-resumen", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK, "{resumen}");
    assert_eq!(resumen["facturado_centavos"], 3500, "la anulada no factura: {resumen}");
    assert_eq!(resumen["tickets"], 2);
    assert_eq!(resumen["ticket_promedio_centavos"], 1750);
    assert_eq!(resumen["anuladas"], 1);
    assert_eq!(resumen["anuladas_centavos"], 9000, "la anulada de 9000 sí cuenta acá: {resumen}");

    // Sin costo cargado, el margen es 100% del facturado.
    assert_eq!(resumen["costo_vendido_centavos"], 0);
    assert_eq!(resumen["margen_centavos"], 3500);

    let motivos = resumen["anuladas_por_motivo"].as_array().unwrap();
    assert_eq!(motivos.len(), 1);
    assert_eq!(motivos[0]["total_centavos"], 9000);

    let medios = resumen["por_medio"].as_array().unwrap();
    assert_eq!(medios.len(), 2);
    let efectivo = medios.iter().find(|m| m["medio"] == "efectivo").unwrap();
    assert_eq!(efectivo["total_centavos"], 1000);

    // Top productos: facturado solo de confirmadas.
    let (st, top) = pedir(&app, "GET", "/reportes/top-productos", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    let top = top.as_array().unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0]["facturado_centavos"], 3500);

    // Rendimiento por vendedor: mismo admin operó las tres ventas.
    let (st, vendedores) = pedir(&app, "GET", "/reportes/ventas-por-vendedor", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK, "{vendedores}");
    let vendedores = vendedores.as_array().unwrap();
    assert_eq!(vendedores.len(), 1);
    assert_eq!(vendedores[0]["tickets"], 2);
    assert_eq!(vendedores[0]["facturado_centavos"], 3500);
    assert_eq!(vendedores[0]["anuladas"], 1);

    // Arqueos: tras cerrar la sesión aparece con su diferencia.
    let (st, _) = pedir(&app, "POST", &format!("/ventas/sesiones/{sesion_id}/cerrar"),
        Some(&token), Some(json!({ "monto_contado_centavos": 900 }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, arqueos) = pedir(&app, "GET", "/reportes/arqueos", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    let sesiones = arqueos["sesiones"].as_array().unwrap();
    assert_eq!(sesiones.len(), 1);
    assert_eq!(sesiones[0]["diferencia_centavos"], -100, "esperado 1000 de efectivo, contado 900");
    assert_eq!(arqueos["total_diferencia_centavos"], -100);
    assert_eq!(arqueos["con_diferencia"], 1);

    // Inventario: la venta dejó stock negativo, que se señala pero NO
    // entra en la valuación (un faltante no es valor).
    let (st, inv) = pedir(&app, "GET", "/reportes/inventario", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(inv["productos_con_stock_negativo"], 1, "{inv}");
    assert_eq!(inv["valor_a_precio_centavos"], 0, "el stock negativo no vale negativo: {inv}");
    assert_eq!(inv["valor_a_costo_centavos"], 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn reportes_exigen_permiso(pool: PgPool) {
    let cajero = crear_usuario_con_rol(&pool, "cajero", ROL_CAJERO_ID).await;
    let token = token_para(cajero);
    let app = app(pool);

    for ruta in [
        "/reportes/ventas-resumen",
        "/reportes/fiado",
        "/reportes/inventario",
        "/reportes/productos-sin-movimiento",
        "/reportes/ventas-por-vendedor",
        "/reportes/mermas",
        "/reportes/arqueos",
        "/reportes/compras-resumen",
    ] {
        let (st, _) = pedir(&app, "GET", ruta, Some(&token), None).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{ruta}");
    }
}
