// Caché local del catálogo vendible. Se refresca del servidor cuando hay
// conexión; la caja busca y escanea contra esta copia cuando no la hay.

import { api } from './api';
import { db, type ProductoCaja } from './db';

interface RespuestaSync {
  generado_en: string;
  productos: ProductoCaja[];
}

/** Baja el catálogo completo y reemplaza la copia local. */
export async function sincronizarCatalogo(): Promise<void> {
  const r = await api<RespuestaSync>('GET', '/catalogo/sincronizacion-caja');
  const base = await db();
  const tx = base.transaction(['productos', 'codigos', 'meta'], 'readwrite');
  await tx.objectStore('productos').clear();
  await tx.objectStore('codigos').clear();
  for (const p of r.productos) {
    await tx.objectStore('productos').put(p);
    for (const codigo of p.codigos_barras) {
      await tx.objectStore('codigos').put({ codigo, producto_id: p.id });
    }
  }
  await tx.objectStore('meta').put(r.generado_en, 'catalogo_sincronizado_en');
  await tx.done;
}

/**
 * Agrega/actualiza un único producto (y sus códigos) en la copia local sin
 * bajar el catálogo entero — para cuando se lo acaba de crear o asociarle un
 * código desde la caja (alta rápida). Evita un `sincronizarCatalogo()`
 * completo (fetch + clear + reinserción de todo el catálogo) por cada alta.
 */
export async function upsertProductoLocal(p: ProductoCaja): Promise<void> {
  const base = await db();
  const tx = base.transaction(['productos', 'codigos'], 'readwrite');
  await tx.objectStore('productos').put(p);
  for (const codigo of p.codigos_barras) {
    await tx.objectStore('codigos').put({ codigo, producto_id: p.id });
  }
  await tx.done;
}

export async function ultimaSincronizacion(): Promise<string | undefined> {
  return (await (await db()).get('meta', 'catalogo_sincronizado_en')) as string | undefined;
}

/** Búsqueda por nombre sobre la copia local (subcadena, sin acentos rigurosos). */
export async function buscarLocal(termino: string, limite = 6): Promise<ProductoCaja[]> {
  const todos = await (await db()).getAll('productos');
  const aguja = termino.trim().toLowerCase();
  if (!aguja) return [];
  return todos
    .filter((p) => p.nombre.toLowerCase().includes(aguja))
    .slice(0, limite);
}

export async function porCodigoLocal(codigo: string): Promise<ProductoCaja | undefined> {
  const base = await db();
  const entrada = await base.get('codigos', codigo);
  if (!entrada) return undefined;
  return base.get('productos', entrada.producto_id);
}
