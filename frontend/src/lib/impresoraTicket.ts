// Impresión de tickets por WebUSB: el navegador le habla directo a la
// impresora térmica conectada por USB al dispositivo del mostrador. No hay
// backend en el medio (el server puede vivir en otra máquina de la red) ni
// diálogo de impresión — solo funciona en Chrome/Edge y en un contexto
// seguro (HTTPS o localhost); no existe en Safari/iOS.
//
// El vendorId/productId de la 3nStar RPT004 no está confirmado, así que
// `vincularImpresora` deja elegir cualquier USB conectado. Tampoco se pudo
// probar contra el hardware real: si `claimInterface`/el endpoint de salida
// no son los esperados, loguear `dispositivo.configuration` para ajustarlos.

import type { MedioPago } from './api';
import { cantidad, fechaHora, pesos } from './formato';

const ANCHO = 48; // columnas a 80mm

const ETIQUETAS_MEDIO: Record<MedioPago, string> = {
  efectivo: 'Efectivo',
  tarjeta: 'Tarjeta',
  mercado_pago: 'Mercado Pago',
  transferencia: 'Transferencia',
  cuenta_corriente: 'Cuenta corriente',
};

export interface ItemTicket {
  nombre: string;
  cantidad: number;
  precioUnitarioCentavos: number;
  subtotalCentavos: number;
}

export interface PagoTicket {
  medio: MedioPago;
  montoCentavos: number;
}

export interface DatosTicket {
  encabezado: string;
  pie: string;
  items: ItemTicket[];
  pagos: PagoTicket[];
  totalCentavos: number;
  descuentoCentavos: number;
  vendidaEn: string;
}

let dispositivo: USBDevice | null = null;
let interfazReclamada = false;

function soportaWebUsb(): boolean {
  return typeof navigator !== 'undefined' && navigator.usb !== undefined;
}

/** Si ya hay una impresora vinculada en este navegador (sin volver a preguntar). */
export async function impresoraVinculada(): Promise<boolean> {
  if (!soportaWebUsb()) return false;
  const dispositivos = await navigator.usb!.getDevices();
  return dispositivos.length > 0;
}

/**
 * Abre el selector de dispositivos USB del navegador. Tiene que dispararse
 * directo desde un click de usuario (no se puede llamar automáticamente) —
 * se usa solo desde el botón "Vincular impresora" de Configuración del ticket.
 */
export async function vincularImpresora(): Promise<void> {
  if (!soportaWebUsb()) {
    throw new Error('Este navegador no soporta WebUSB — usá Chrome o Edge, con HTTPS.');
  }
  dispositivo = await navigator.usb!.requestDevice({ filters: [] });
  interfazReclamada = false;
}

async function dispositivoListo(): Promise<USBDevice> {
  if (!soportaWebUsb()) {
    throw new Error('Este navegador no soporta WebUSB — usá Chrome o Edge, con HTTPS.');
  }
  if (!dispositivo) {
    const [primero] = await navigator.usb!.getDevices();
    if (!primero) {
      throw new Error('No hay impresora vinculada — configurala en Productos → Configuración del ticket.');
    }
    dispositivo = primero;
  }
  if (!interfazReclamada) {
    await dispositivo.open();
    if (!dispositivo.configuration) await dispositivo.selectConfiguration(1);
    await dispositivo.claimInterface(0);
    interfazReclamada = true;
  }
  return dispositivo;
}

function endpointSalida(dev: USBDevice): number {
  const alterna = dev.configuration?.interfaces[0]?.alternate;
  const endpoint = alterna?.endpoints.find((e) => e.direction === 'out');
  return endpoint?.endpointNumber ?? 1;
}

/** Manda los bytes ESC/POS ya armados (ver `armarTicketEscPos`) a la impresora vinculada. */
export async function imprimir(bytes: Uint8Array): Promise<void> {
  const dev = await dispositivoListo();
  await dev.transferOut(endpointSalida(dev), bytes);
}

// ---------- Armado del ticket ESC/POS ----------

const ESC_INIT = new Uint8Array([0x1b, 0x40]);
const ESC_CENTRAR = new Uint8Array([0x1b, 0x61, 0x01]);
const ESC_IZQUIERDA = new Uint8Array([0x1b, 0x61, 0x00]);
const ESC_NEGRITA_ON = new Uint8Array([0x1b, 0x45, 0x01]);
const ESC_NEGRITA_OFF = new Uint8Array([0x1b, 0x45, 0x00]);
// 3 saltos de línea (deja lugar para cortar con margen) + corte total.
const CORTE = new Uint8Array([0x0a, 0x0a, 0x0a, 0x1d, 0x56, 0x00]);

/** Quita acentos: no sabemos si la impresora real interpreta UTF-8 o una code page. */
function limpiar(texto: string): string {
  return texto.normalize('NFD').replace(/[\u0300-\u036f]/g, '');
}

function envolver(texto: string, ancho: number): string[] {
  const palabras = limpiar(texto).split(/\s+/).filter(Boolean);
  const lineas: string[] = [];
  let actual = '';
  for (const palabra of palabras) {
    const candidata = actual ? `${actual} ${palabra}` : palabra;
    if (candidata.length > ancho) {
      if (actual) lineas.push(actual);
      actual = palabra.slice(0, ancho);
    } else {
      actual = candidata;
    }
  }
  if (actual) lineas.push(actual);
  return lineas.length > 0 ? lineas : [''];
}

function centrado(texto: string, ancho: number): string {
  const relleno = Math.max(0, ancho - texto.length);
  return ' '.repeat(Math.floor(relleno / 2)) + texto;
}

function dosColumnas(izquierda: string, derecha: string, ancho: number): string {
  const espacio = Math.max(1, ancho - izquierda.length - derecha.length);
  return izquierda + ' '.repeat(espacio) + derecha;
}

export function armarTicketEscPos(datos: DatosTicket): Uint8Array {
  const codificador = new TextEncoder();
  const partes: Uint8Array[] = [ESC_INIT];
  const agregar = (linea: string) => partes.push(codificador.encode(`${limpiar(linea)}\n`));

  partes.push(ESC_CENTRAR);
  const encabezado = datos.encabezado.trim();
  if (encabezado) {
    partes.push(ESC_NEGRITA_ON);
    for (const l of encabezado.split('\n')) agregar(centrado(l, ANCHO));
    partes.push(ESC_NEGRITA_OFF);
    agregar('');
  }
  agregar(fechaHora(datos.vendidaEn));
  partes.push(ESC_IZQUIERDA);
  agregar('-'.repeat(ANCHO));

  for (const item of datos.items) {
    for (const l of envolver(item.nombre, ANCHO)) agregar(l);
    const detalle = `${cantidad(item.cantidad)} x ${pesos(item.precioUnitarioCentavos)}`;
    agregar(dosColumnas(detalle, pesos(item.subtotalCentavos), ANCHO));
  }
  agregar('-'.repeat(ANCHO));

  if (datos.descuentoCentavos > 0) {
    agregar(dosColumnas('Descuento', `-${pesos(datos.descuentoCentavos)}`, ANCHO));
  }
  partes.push(ESC_NEGRITA_ON);
  agregar(dosColumnas('TOTAL', pesos(datos.totalCentavos), ANCHO));
  partes.push(ESC_NEGRITA_OFF);
  agregar('');

  for (const pago of datos.pagos) {
    agregar(dosColumnas(ETIQUETAS_MEDIO[pago.medio] ?? pago.medio, pesos(pago.montoCentavos), ANCHO));
  }

  const pie = datos.pie.trim();
  if (pie) {
    agregar('');
    partes.push(ESC_CENTRAR);
    for (const l of pie.split('\n')) agregar(centrado(l, ANCHO));
  }

  partes.push(CORTE);

  const total = partes.reduce((n, p) => n + p.length, 0);
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const parte of partes) {
    bytes.set(parte, offset);
    offset += parte.length;
  }
  return bytes;
}
