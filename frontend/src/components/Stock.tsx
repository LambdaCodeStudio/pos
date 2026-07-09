// Stock: alertas de vencimiento accionables, consulta por producto con sus
// lotes, y ajustes (conteo o delta) con los 6 motivos del dominio.

import { useCallback, useEffect, useRef, useState } from 'react';
import { api, tienePermiso, type AlertaVencimiento, type Producto, type StockDeProducto } from '../lib/api';
import { cantidad as fmtCantidad, fecha } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

const MOTIVOS = [
  { valor: 'conteo', etiqueta: 'Conteo físico' },
  { valor: 'rotura', etiqueta: 'Rotura' },
  { valor: 'vencimiento', etiqueta: 'Vencimiento (se tira)' },
  { valor: 'perdida', etiqueta: 'Pérdida' },
  { valor: 'robo', etiqueta: 'Robo (descubierto en conteo)' },
  { valor: 'otro', etiqueta: 'Otro' },
] as const;

export default function Stock() {
  const [alertas, setAlertas] = useState<AlertaVencimiento[] | null>(null);
  const [dias, setDias] = useState(30);
  const [ajustando, setAjustando] = useState<{ producto: Producto; loteId?: string } | null>(null);
  const [refresco, setRefresco] = useState(0);

  useEffect(() => {
    api<AlertaVencimiento[]>('GET', `/inventario/alertas-vencimiento?dias=${dias}`)
      .then(setAlertas).catch(() => setAlertas([]));
  }, [dias, refresco]);

  const puedeAjustar = tienePermiso('ajustar_stock');

  return (
    <Shell seccion="/stock">
      <Encabezado titulo="Stock" subtitulo="Alertas de vencimiento, consulta por producto y ajustes." />

      <div className="grid gap-5 lg:grid-cols-2">
        <Tarjeta
          titulo="Por vencer"
          accion={
            <select className="rounded-lg border border-stone-200 px-2 py-1 text-xs text-stone-500"
              value={dias} onChange={(e) => setDias(Number(e.target.value))}>
              <option value={7}>7 días</option>
              <option value={15}>15 días</option>
              <option value={30}>30 días</option>
              <option value={60}>60 días</option>
            </select>
          }
        >
          {alertas === null ? (
            <Cargando />
          ) : alertas.length === 0 ? (
            <EstadoVacio mensaje="Nada por vencer en esa ventana." />
          ) : (
            <ul className="divide-y divide-stone-100">
              {alertas.map((a) => (
                <li key={a.lote_id} className="flex items-center justify-between gap-3 py-2.5">
                  <div>
                    <p className="text-sm font-medium text-stone-800">{a.producto_nombre}</p>
                    <p className="text-xs text-stone-400">
                      {fmtCantidad(a.cantidad_actual)} u. · vence {fecha(a.vencimiento)}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Insignia tono={a.dias_restantes <= 3 ? 'rojo' : a.dias_restantes <= 7 ? 'ambar' : 'neutro'}>
                      {a.dias_restantes <= 0 ? 'vencido' : `${a.dias_restantes} días`}
                    </Insignia>
                    {puedeAjustar && (
                      <Boton chico variante="fantasma" onClick={async () => {
                        const p = await api<Producto>('GET', `/catalogo/productos/${a.producto_id}`);
                        setAjustando({ producto: p, loteId: a.lote_id });
                      }}>
                        Ajustar
                      </Boton>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Tarjeta>

        <ConsultaStock puedeAjustar={puedeAjustar} onAjustar={(producto) => setAjustando({ producto })} refresco={refresco} />
      </div>

      {ajustando && (
        <ModalAjuste
          producto={ajustando.producto}
          loteId={ajustando.loteId}
          onCerrar={() => setAjustando(null)}
          onGuardado={() => { setAjustando(null); setRefresco((n) => n + 1); }}
        />
      )}
    </Shell>
  );
}

function ConsultaStock({
  puedeAjustar,
  onAjustar,
  refresco,
}: {
  puedeAjustar: boolean;
  onAjustar: (p: Producto) => void;
  refresco: number;
}) {
  const [busqueda, setBusqueda] = useState('');
  const [resultados, setResultados] = useState<Producto[]>([]);
  const [producto, setProducto] = useState<Producto | null>(null);
  const [stock, setStock] = useState<StockDeProducto | null>(null);
  const temporizador = useRef<number | undefined>(undefined);

  const cargarStock = useCallback((p: Producto) => {
    api<StockDeProducto>('GET', `/inventario/productos/${p.id}/stock`).then(setStock).catch(() => {});
  }, []);

  useEffect(() => {
    if (producto) cargarStock(producto);
  }, [producto, cargarStock, refresco]);

  function alEscribir(valor: string) {
    setBusqueda(valor);
    window.clearTimeout(temporizador.current);
    if (valor.trim().length < 2) { setResultados([]); return; }
    temporizador.current = window.setTimeout(async () => {
      setResultados(await api<Producto[]>('GET', `/catalogo/productos?buscar=${encodeURIComponent(valor.trim())}&limite=6`));
    }, 200);
  }

  return (
    <Tarjeta titulo="Consultar producto">
      <div className="relative">
        <input className={claseInput} placeholder="Buscar producto…" value={busqueda}
          onChange={(e) => alEscribir(e.target.value)} />
        {resultados.length > 0 && (
          <ul className="absolute z-10 mt-1 w-full divide-y divide-stone-100 overflow-hidden rounded-lg border border-stone-200 bg-white shadow-lg">
            {resultados.map((p) => (
              <li key={p.id}>
                <button type="button" className="w-full px-4 py-2.5 text-left text-sm font-medium text-stone-800 hover:bg-acento-50"
                  onClick={() => { setProducto(p); setResultados([]); setBusqueda(p.nombre); }}>
                  {p.nombre}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {producto && stock && (
        <div className="mt-5">
          <div className="flex items-end justify-between">
            <div>
              <p className="text-sm text-stone-500">{producto.nombre}</p>
              <p className="text-3xl font-bold text-stone-900">
                {fmtCantidad(stock.cantidad)}
                <span className="ml-1 text-sm font-normal text-stone-400">
                  {producto.unidad_de_venta === 'peso' ? 'kg' : 'unidades'}
                </span>
              </p>
              {parseFloat(stock.cantidad) < 0 && (
                <p className="text-xs text-red-500">Stock negativo: la caja nunca bloquea; recalibralo con un conteo.</p>
              )}
            </div>
            {puedeAjustar && <Boton chico variante="secundario" onClick={() => onAjustar(producto)}>Ajustar</Boton>}
          </div>

          {stock.lotes.length > 0 && (
            <div className="mt-4">
              <Tabla encabezados={['Lote', 'Vencimiento', 'Cantidad']}>
                {stock.lotes.map((l) => (
                  <tr key={l.id}>
                    <td className="px-3 py-2 text-stone-500">{l.codigo_lote ?? l.id.slice(0, 8)}</td>
                    <td className="px-3 py-2 text-stone-500">{fecha(l.vencimiento)}</td>
                    <td className="px-3 py-2 font-medium text-stone-800">{fmtCantidad(l.cantidad_actual)}</td>
                  </tr>
                ))}
              </Tabla>
            </div>
          )}
        </div>
      )}
    </Tarjeta>
  );
}

function ModalAjuste({
  producto,
  loteId,
  onCerrar,
  onGuardado,
}: {
  producto: Producto;
  loteId?: string;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [motivo, setMotivo] = useState<(typeof MOTIVOS)[number]['valor']>(loteId ? 'vencimiento' : 'conteo');
  const [modo, setModo] = useState<'contada' | 'delta'>(loteId ? 'delta' : 'contada');
  const [valor, setValor] = useState('');
  const [observaciones, setObservaciones] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const numero = valor.trim().replace(',', '.');
    if (numero === '' || !Number.isFinite(Number(numero))) { setError('Cantidad inválida'); return; }
    try {
      await api('POST', '/inventario/ajustes', {
        motivo,
        observaciones: observaciones || null,
        items: [{
          producto_id: producto.id,
          lote_id: loteId ?? null,
          ...(modo === 'contada' ? { cantidad_contada: numero } : { delta: numero }),
        }],
      });
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={`Ajustar ${producto.nombre}`} onCerrar={onCerrar} ancho="max-w-md">
      <form onSubmit={guardar} className="space-y-4">
        {loteId && <p className="rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">Ajuste sobre un lote específico.</p>}
        <Campo etiqueta="Motivo">
          <select className={claseInput} value={motivo} onChange={(e) => setMotivo(e.target.value as typeof motivo)}>
            {MOTIVOS.map((m) => (
              <option key={m.valor} value={m.valor}>{m.etiqueta}</option>
            ))}
          </select>
        </Campo>
        <div className="grid grid-cols-2 rounded-lg bg-stone-100 p-1 text-sm font-medium">
          {(['contada', 'delta'] as const).map((m) => (
            <button key={m} type="button" onClick={() => setModo(m)}
              className={`rounded-md py-1.5 transition ${modo === m ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500'}`}>
              {m === 'contada' ? 'Conté y hay…' : 'Sumar / restar'}
            </button>
          ))}
        </div>
        <Campo
          etiqueta={modo === 'contada' ? 'Cantidad contada' : 'Delta (negativo resta, ej: -3)'}
          ayuda={modo === 'contada' ? 'El sistema calcula la diferencia contra la proyección' : 'Un ajuste negativo no puede dejar el stock bajo cero'}
        >
          <input className={claseInput + ' text-lg font-semibold'} value={valor}
            onChange={(e) => setValor(e.target.value)} inputMode="text" autoFocus />
        </Campo>
        <Campo etiqueta="Observaciones">
          <input className={claseInput} value={observaciones} onChange={(e) => setObservaciones(e.target.value)} />
        </Campo>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit">Aplicar ajuste</Boton>
        </div>
      </form>
    </Modal>
  );
}
