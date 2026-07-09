// Cola de sincronización offline-first. Las escrituras de la caja (abrir
// sesión, ventas) se encolan con su UUID y se empujan al servidor EN ORDEN
// cuando hay conexión. El backend es idempotente por UUID: reintentar jamás
// duplica. Un rechazo de negocio (4xx) marca la operación en error y NO
// frena la cola; un fallo de red la deja pendiente y corta hasta reconectar.

import { api, ErrorApi } from './api';
import { db, type OperacionPendiente } from './db';

type Oyente = () => void;
const oyentes = new Set<Oyente>();
let procesando = false;
let iniciado = false;

export function alCambiarCola(oyente: Oyente): () => void {
  oyentes.add(oyente);
  return () => oyentes.delete(oyente);
}

function avisar() {
  for (const oyente of oyentes) oyente();
}

export async function encolar(
  op: Omit<OperacionPendiente, 'secuencia' | 'creado_en' | 'estado'>,
): Promise<void> {
  const base = await db();
  await base.put('cola', {
    ...op,
    secuencia: Date.now(),
    creado_en: new Date().toISOString(),
    estado: 'pendiente',
  });
  avisar();
  void procesarCola();
}

export interface EstadoCola {
  pendientes: number;
  con_error: number;
}

export async function estadoCola(): Promise<EstadoCola> {
  const todas = await (await db()).getAll('cola');
  return {
    pendientes: todas.filter((o) => o.estado === 'pendiente').length,
    con_error: todas.filter((o) => o.estado === 'error').length,
  };
}

export async function operacionesConError(): Promise<OperacionPendiente[]> {
  const todas = await (await db()).getAll('cola');
  return todas.filter((o) => o.estado === 'error');
}

export async function descartarOperacion(id: string): Promise<void> {
  await (await db()).delete('cola', id);
  avisar();
}

/** Empuja la cola en orden. Corta ante el primer fallo de red. */
export async function procesarCola(): Promise<void> {
  if (procesando || !navigator.onLine) return;
  procesando = true;
  try {
    const base = await db();
    const operaciones = (await base.getAllFromIndex('cola', 'por_secuencia'))
      .filter((o) => o.estado === 'pendiente');

    for (const op of operaciones) {
      try {
        await api(op.metodo, op.ruta, op.cuerpo);
        await base.delete('cola', op.id);
        avisar();
      } catch (error) {
        if (error instanceof ErrorApi && error.status !== 401) {
          // Rechazo de negocio: queda para revisión manual, la cola sigue.
          op.estado = 'error';
          op.error = error.message;
          await base.put('cola', op);
          avisar();
          continue;
        }
        // Fallo de red (o 401 que ya redirigió al login): reintento después.
        break;
      }
    }
  } finally {
    procesando = false;
  }
}

/** Arranca los disparadores de sincronización (una sola vez por pestaña). */
export function iniciarSincronizacion(): void {
  if (iniciado) return;
  iniciado = true;
  window.addEventListener('online', () => void procesarCola());
  window.setInterval(() => void procesarCola(), 30_000);
  void procesarCola();
}
