pub mod rutas;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ErrorApi;

#[derive(sqlx::Type, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[sqlx(type_name = "catalogo.unidad_de_venta", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum UnidadDeVenta {
    Unidad,
    Peso,
}

/// Redondeo comercial configurado para el precio de venta calculado en
/// recepciones, en centavos (0 = sin redondeo). Vive en
/// `catalogo.configuracion` y se cambia desde la API.
pub async fn redondeo_precio_configurado<'e, E>(ejecutor: E) -> Result<i64, ErrorApi>
where
    E: sqlx::PgExecutor<'e>,
{
    let valor = sqlx::query_scalar!(
        r#"SELECT valor FROM catalogo.configuracion WHERE clave = 'redondeo_precio_centavos'"#,
    )
    .fetch_optional(ejecutor)
    .await?;
    Ok(valor.and_then(|v| v.as_i64()).unwrap_or(0))
}

/// Datos del producto que necesita Compras para cargar un ítem de recepción.
pub struct ProductoParaCompra {
    pub id: Uuid,
    pub nombre: String,
    pub markup_pct: Decimal,
    pub iva_pct: Decimal,
    pub controla_vencimiento: bool,
}

/// Cascada de resolución de markup/IVA: override del producto → default de la
/// categoría DIRECTA (la herencia nunca sube por el árbol). El valor explícito
/// del documento, si existe, se resuelve en el llamador.
pub async fn producto_para_compra<'e, E>(
    ejecutor: E,
    producto_id: Uuid,
) -> Result<ProductoParaCompra, ErrorApi>
where
    E: sqlx::PgExecutor<'e>,
{
    let fila = sqlx::query!(
        r#"
        SELECT p.id, p.nombre,
               COALESCE(p.markup_pct_override, c.markup_pct) AS "markup_pct!",
               COALESCE(p.iva_pct_override, c.iva_pct) AS "iva_pct!",
               p.controla_vencimiento
        FROM catalogo.productos p
        JOIN catalogo.categorias c ON c.id = p.categoria_id
        WHERE p.id = $1 AND p.activo
        "#,
        producto_id,
    )
    .fetch_optional(ejecutor)
    .await?
    .ok_or_else(|| ErrorApi::Validacion("producto inexistente o inactivo".into()))?;

    Ok(ProductoParaCompra {
        id: fila.id,
        nombre: fila.nombre,
        markup_pct: fila.markup_pct,
        iva_pct: fila.iva_pct,
        controla_vencimiento: fila.controla_vencimiento,
    })
}
