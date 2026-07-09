// Lista de recepciones con su estado en el flujo
// borrador → confirmada → completada (etiquetado terminado).

import { useCallback, useEffect, useState } from 'react';
import { api, type EstadoRecepcion, type Proveedor, type RecepcionResumen } from '../lib/api';
import { fechaHora } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

const TONO_ESTADO: Record<EstadoRecepcion, 'neutro' | 'ambar' | 'verde'> = {
  borrador: 'neutro',
  confirmada: 'ambar',
  completada: 'verde',
};

export default function Recepciones() {
  const [recepciones, setRecepciones] = useState<RecepcionResumen[] | null>(null);
  const [filtro, setFiltro] = useState<EstadoRecepcion | ''>('');
  const [creando, setCreando] = useState(false);

  const cargar = useCallback(() => {
    const q = filtro ? `?estado=${filtro}` : '';
    api<RecepcionResumen[]>('GET', `/compras/recepciones${q}`).then(setRecepciones).catch(() => setRecepciones([]));
  }, [filtro]);
  useEffect(() => cargar(), [cargar]);

  return (
    <Shell seccion="/recepciones">
      <Encabezado
        titulo="Recepciones"
        subtitulo="Llegó mercadería: cargala, confirmá y salí a etiquetar."
        accion={<Boton onClick={() => setCreando(true)}>+ Nueva recepción</Boton>}
      />

      <Tarjeta>
        <div className="mb-4 flex gap-2">
          {(['', 'borrador', 'confirmada', 'completada'] as const).map((e) => (
            <button
              key={e}
              onClick={() => setFiltro(e)}
              className={`rounded-full px-3.5 py-1.5 text-xs font-medium transition ${
                filtro === e ? 'bg-stone-800 text-white' : 'bg-stone-100 text-stone-500 hover:bg-stone-200'
              }`}
            >
              {e === '' ? 'Todas' : e}
            </button>
          ))}
        </div>

        {recepciones === null ? (
          <Cargando />
        ) : recepciones.length === 0 ? (
          <EstadoVacio mensaje="No hay recepciones con ese filtro." />
        ) : (
          <Tabla encabezados={['Proveedor', 'Estado', 'Ítems', 'Etiquetado', 'Creada', '']}>
            {recepciones.map((r) => (
              <tr key={r.id} className="hover:bg-stone-50">
                <td className="px-3 py-3 font-medium text-stone-800">
                  {r.proveedor_nombre ?? <span className="text-stone-400">Sin proveedor</span>}
                </td>
                <td className="px-3 py-3"><Insignia tono={TONO_ESTADO[r.estado]}>{r.estado}</Insignia></td>
                <td className="px-3 py-3 text-stone-500">{r.cantidad_items}</td>
                <td className="px-3 py-3 text-stone-500">
                  {r.estado === 'borrador'
                    ? '—'
                    : r.items_pendientes_etiquetar === 0
                      ? '✓ completo'
                      : `${r.items_pendientes_etiquetar} pendientes`}
                </td>
                <td className="px-3 py-3 text-stone-400">{fechaHora(r.creada_en)}</td>
                <td className="px-3 py-3 text-right">
                  <a href={`/recepcion?id=${r.id}`} className="text-sm font-medium text-acento-700 hover:underline">
                    Abrir →
                  </a>
                </td>
              </tr>
            ))}
          </Tabla>
        )}
      </Tarjeta>

      {creando && <ModalNuevaRecepcion onCerrar={() => setCreando(false)} />}
    </Shell>
  );
}

function ModalNuevaRecepcion({ onCerrar }: { onCerrar: () => void }) {
  const [proveedores, setProveedores] = useState<Proveedor[]>([]);
  const [proveedorId, setProveedorId] = useState('');
  const [observaciones, setObservaciones] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<Proveedor[]>('GET', '/compras/proveedores').then(setProveedores).catch(() => {});
  }, []);

  async function crear(e: React.FormEvent) {
    e.preventDefault();
    try {
      const r = await api<{ id: string }>('POST', '/compras/recepciones', {
        proveedor_id: proveedorId || null,
        observaciones: observaciones || null,
      });
      window.location.href = `/recepcion?id=${r.id}`;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo="Nueva recepción" onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={crear} className="space-y-4">
        <Campo etiqueta="Proveedor (opcional)">
          <select className={claseInput} value={proveedorId} onChange={(e) => setProveedorId(e.target.value)}>
            <option value="">— Sin proveedor —</option>
            {proveedores.map((p) => (
              <option key={p.id} value={p.id}>{p.nombre}</option>
            ))}
          </select>
        </Campo>
        <Campo etiqueta="Observaciones">
          <textarea className={claseInput} rows={2} value={observaciones} onChange={(e) => setObservaciones(e.target.value)} />
        </Campo>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit">Crear y cargar ítems</Boton>
        </div>
      </form>
    </Modal>
  );
}
