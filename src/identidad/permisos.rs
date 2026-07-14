//! Catálogo FIJO de permisos, definido en código y versionado con el software.
//! NUNCA creables desde la UI. La base solo almacena estos nombres.

pub const VENDER: &str = "vender";
pub const ANULAR_VENTA: &str = "anular_venta";
pub const APLICAR_DESCUENTO: &str = "aplicar_descuento";
pub const EXCEDER_LIMITE_CREDITO: &str = "exceder_limite_credito";
pub const CONFIRMAR_RECEPCION: &str = "confirmar_recepcion";
pub const AJUSTAR_STOCK: &str = "ajustar_stock";
pub const MODIFICAR_PRECIOS: &str = "modificar_precios";
pub const GESTIONAR_USUARIOS: &str = "gestionar_usuarios";
pub const GESTIONAR_CLIENTES: &str = "gestionar_clientes";
pub const VER_REPORTES: &str = "ver_reportes";
pub const CERRAR_CAJA: &str = "cerrar_caja";
pub const ABRIR_CAJA: &str = "abrir_caja";
pub const GESTIONAR_CATALOGO: &str = "gestionar_catalogo";
pub const GESTIONAR_PROVEEDORES: &str = "gestionar_proveedores";
/// Único permiso con el que actúa un dispositivo autenticado por HMAC
/// (contexto Identidad, ver `identidad::dispositivos`), nunca asignado a un
/// rol de usuario.
pub const ETIQUETAR: &str = "etiquetar";

pub const TODOS: &[&str] = &[
    VENDER,
    ANULAR_VENTA,
    APLICAR_DESCUENTO,
    EXCEDER_LIMITE_CREDITO,
    CONFIRMAR_RECEPCION,
    AJUSTAR_STOCK,
    MODIFICAR_PRECIOS,
    GESTIONAR_USUARIOS,
    GESTIONAR_CLIENTES,
    VER_REPORTES,
    CERRAR_CAJA,
    ABRIR_CAJA,
    GESTIONAR_CATALOGO,
    GESTIONAR_PROVEEDORES,
    ETIQUETAR,
];

pub fn es_valido(permiso: &str) -> bool {
    TODOS.contains(&permiso)
}
