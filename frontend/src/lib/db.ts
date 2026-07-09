// Base local (IndexedDB) de la PWA de caja:
//  - productos/codigos: caché del catálogo vendible para operar sin red.
//  - cola: operaciones de escritura pendientes de sincronizar (FIFO).
//  - meta: clave-valor (última sincronización, sesión de caja local).

import { openDB, type DBSchema, type IDBPDatabase } from 'idb';

export interface ProductoCaja {
  id: string;
  nombre: string;
  unidad_de_venta: 'unidad' | 'peso';
  precio_actual_centavos: number | null;
  iva_pct: string;
  codigos_barras: string[];
}

export interface OperacionPendiente {
  /** UUID de la operación (el mismo que garantiza idempotencia en el server). */
  id: string;
  /** Orden de encolado: se sincroniza estrictamente en este orden. */
  secuencia: number;
  descripcion: string;
  metodo: 'POST';
  ruta: string;
  cuerpo: unknown;
  creado_en: string;
  /** 'pendiente' se reintenta; 'error' (4xx del server) espera revisión. */
  estado: 'pendiente' | 'error';
  error?: string;
}

interface EsquemaPos extends DBSchema {
  productos: { key: string; value: ProductoCaja };
  codigos: { key: string; value: { codigo: string; producto_id: string } };
  cola: {
    key: string;
    value: OperacionPendiente;
    indexes: { por_secuencia: number };
  };
  meta: { key: string; value: unknown };
}

let instancia: Promise<IDBPDatabase<EsquemaPos>> | null = null;

export function db(): Promise<IDBPDatabase<EsquemaPos>> {
  if (!instancia) {
    instancia = openDB<EsquemaPos>('pos-caja', 1, {
      upgrade(base) {
        base.createObjectStore('productos', { keyPath: 'id' });
        base.createObjectStore('codigos', { keyPath: 'codigo' });
        const cola = base.createObjectStore('cola', { keyPath: 'id' });
        cola.createIndex('por_secuencia', 'secuencia');
        base.createObjectStore('meta');
      },
    });
  }
  return instancia;
}

export async function leerMeta<T>(clave: string): Promise<T | undefined> {
  return (await (await db()).get('meta', clave)) as T | undefined;
}

export async function escribirMeta(clave: string, valor: unknown): Promise<void> {
  await (await db()).put('meta', valor, clave);
}

export async function borrarMeta(clave: string): Promise<void> {
  await (await db()).delete('meta', clave);
}
