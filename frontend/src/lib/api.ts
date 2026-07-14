// Cliente HTTP hacia el backend. El token JWT vive en localStorage; un 401
// en cualquier request manda de vuelta al login.

const BASE = '/api';
const CLAVE_TOKEN = 'pos_token';
const CLAVE_USUARIO = 'pos_usuario';

export interface Usuario {
  id: string;
  nombre: string;
  permisos: string[];
}

export class ErrorApi extends Error {
  constructor(
    mensaje: string,
    public status: number,
  ) {
    super(mensaje);
  }
}

export function tokenGuardado(): string | null {
  return localStorage.getItem(CLAVE_TOKEN);
}

export function usuarioGuardado(): Usuario | null {
  const crudo = localStorage.getItem(CLAVE_USUARIO);
  return crudo ? (JSON.parse(crudo) as Usuario) : null;
}

export function guardarSesion(token: string, usuario: Usuario) {
  localStorage.setItem(CLAVE_TOKEN, token);
  localStorage.setItem(CLAVE_USUARIO, JSON.stringify(usuario));
}

export function cerrarSesion() {
  localStorage.removeItem(CLAVE_TOKEN);
  localStorage.removeItem(CLAVE_USUARIO);
  window.location.href = '/login';
}

export function tienePermiso(permiso: string): boolean {
  return usuarioGuardado()?.permisos.includes(permiso) ?? false;
}

export async function api<T = unknown>(
  metodo: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  ruta: string,
  cuerpo?: unknown,
): Promise<T> {
  const headers: Record<string, string> = {};
  const token = tokenGuardado();
  if (token) headers['Authorization'] = `Bearer ${token}`;
  if (cuerpo !== undefined) headers['Content-Type'] = 'application/json';

  const respuesta = await fetch(BASE + ruta, {
    method: metodo,
    headers,
    body: cuerpo !== undefined ? JSON.stringify(cuerpo) : undefined,
  });

  if (respuesta.status === 401 && !ruta.startsWith('/identidad/login')) {
    cerrarSesion();
    throw new ErrorApi('sesión vencida', 401);
  }

  const datos = await respuesta.json().catch(() => null);
  if (!respuesta.ok) {
    throw new ErrorApi(
      (datos as { error?: string } | null)?.error ?? 'error inesperado',
      respuesta.status,
    );
  }
  return datos as T;
}

// ---------- Tipos de la API (espejo del backend) ----------
// Los montos son centavos enteros; las cantidades NUMERIC llegan como string.

export interface ConfiguracionNegocio {
  /** Redondeo comercial del precio calculado en recepciones, en centavos
   *  (0 = sin redondeo; 10000 = al múltiplo de $100 más cercano). */
  redondeo_precio_centavos: number;
  /** Texto libre que encabeza el ticket impreso (nombre del local, dirección, etc.). */
  ticket_encabezado: string;
  /** Texto libre al pie del ticket impreso. */
  ticket_pie: string;
}

export interface Categoria {
  id: string;
  nombre: string;
  padre_id: string | null;
  markup_pct: string;
  iva_pct: string;
  activo: boolean;
}

export interface Producto {
  id: string;
  nombre: string;
  categoria_id: string;
  categoria_nombre: string;
  markup_pct_override: string | null;
  iva_pct_override: string | null;
  markup_pct_resuelto: string;
  iva_pct_resuelto: string;
  unidad_de_venta: 'unidad' | 'peso';
  controla_vencimiento: boolean;
  precio_actual_centavos: number | null;
  costo_actual_centavos: number | null;
  activo: boolean;
  codigos_barras: string[];
}

export interface Proveedor {
  id: string;
  nombre: string;
  cuit: string | null;
  telefono: string | null;
  precios_con_iva: boolean;
  condiciones_pago: string | null;
  activo: boolean;
}

export type EstadoRecepcion = 'borrador' | 'confirmada' | 'completada';

export interface RecepcionResumen {
  id: string;
  proveedor_id: string | null;
  proveedor_nombre: string | null;
  estado: EstadoRecepcion;
  observaciones: string | null;
  creada_en: string;
  confirmada_en: string | null;
  completada_en: string | null;
  cantidad_items: number;
  items_pendientes_etiquetar: number;
}

export interface ItemRecepcion {
  id: string;
  producto_id: string;
  producto_nombre: string;
  cantidad: string;
  costo_centavos: number;
  costo_incluye_iva: boolean;
  iva_pct: string;
  markup_pct: string;
  precio_final_centavos: number;
  vencimiento: string | null;
  etiquetado: boolean;
  etiquetado_en: string | null;
}

export interface RecepcionDetalle {
  id: string;
  proveedor_id: string | null;
  proveedor_nombre: string | null;
  estado: EstadoRecepcion;
  observaciones: string | null;
  creada_en: string;
  confirmada_en: string | null;
  completada_en: string | null;
  items: ItemRecepcion[];
}

export interface EtiquetaPendiente {
  item_id: string;
  producto_id: string;
  producto_nombre: string;
  cantidad: string;
  precio_final_centavos: number;
  codigos_barras: string[];
}

export interface AlertaVencimiento {
  lote_id: string;
  producto_id: string;
  producto_nombre: string;
  codigo_lote: string | null;
  vencimiento: string;
  dias_restantes: number;
  cantidad_actual: string;
}

export interface LoteDeProducto {
  id: string;
  codigo_lote: string | null;
  vencimiento: string;
  cantidad_actual: string;
}

export interface StockDeProducto {
  producto_id: string;
  deposito_id: string;
  cantidad: string;
  lotes: LoteDeProducto[];
}

export type MedioPago =
  | 'efectivo'
  | 'tarjeta'
  | 'mercado_pago'
  | 'transferencia'
  | 'cuenta_corriente';

export interface SesionCaja {
  id: string;
  usuario_id: string;
  usuario_nombre: string;
  monto_inicial_centavos: number;
  abierta_en: string;
  cerrada_en: string | null;
  monto_contado_centavos: number | null;
  diferencia_arqueo_centavos: number | null;
  cantidad_ventas: number;
}

export interface VentaResumen {
  id: string;
  sesion_id: string;
  cliente_id: string | null;
  total_centavos: number;
  descuento_centavos: number;
  estado: 'confirmada' | 'anulada';
  usuario_id: string;
  vendida_en: string;
  sincronizada_en: string;
}

export interface Cliente {
  id: string;
  nombre: string;
  telefono: string | null;
  documento: string | null;
  limite_credito_centavos: number | null;
  saldo_actual_centavos: number;
  activo: boolean;
}

export interface ItemCargoFiado {
  producto_id: string;
  producto_nombre: string;
  cantidad: string;
  cantidad_pendiente: string;
}

export interface MovimientoCuenta {
  id: string;
  tipo: 'cargo' | 'pago' | 'ajuste';
  monto_centavos: number;
  venta_id: string | null;
  medio_pago: MedioPago | null;
  motivo: string | null;
  usuario_id: string;
  creado_en: string;
  items: ItemCargoFiado[];
}

export interface UsuarioResumen {
  id: string;
  nombre: string;
  rol_id: string;
  rol_nombre: string;
  tiene_pin: boolean;
  permisos_extra: string[];
  activo: boolean;
}

export interface RolConPermisos {
  id: string;
  nombre: string;
  descripcion: string | null;
  permisos: string[];
  activo: boolean;
}
