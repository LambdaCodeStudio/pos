// Detalle de una recepción: carga de ítems con cálculo de precio en vivo
// (lo calcula el backend con la cascada de markup/IVA), confirmación y el
// recorrido de etiquetado con barra de progreso.

import { useCallback, useEffect, useRef, useState } from 'react';
import { api, ErrorApi, type ConfiguracionNegocio, type Producto, type Proveedor, type RecepcionDetalle as Detalle } from '../lib/api';
import { aCentavos, cantidad as fmtCantidad, desdeCentavos, fecha, pesos, redondearComercial } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

function idDeLaUrl(): string | null {
  return new URLSearchParams(window.location.search).get('id');
}

export default function RecepcionDetalle() {
  const [recepcion, setRecepcion] = useState<Detalle | null>(null);
  const [noEncontrada, setNoEncontrada] = useState(false);

  const cargar = useCallback(() => {
    const id = idDeLaUrl();
    if (!id) { setNoEncontrada(true); return; }
    api<Detalle>('GET', `/compras/recepciones/${id}`).then(setRecepcion).catch(() => setNoEncontrada(true));
  }, []);
  useEffect(() => cargar(), [cargar]);

  return (
    <Shell seccion="/recepciones">
      {noEncontrada ? (
        <EstadoVacio mensaje="Recepción no encontrada." />
      ) : recepcion === null ? (
        <Cargando />
      ) : (
        <Contenido recepcion={recepcion} recargar={cargar} />
      )}
    </Shell>
  );
}

function Contenido({ recepcion, recargar }: { recepcion: Detalle; recargar: () => void }) {
  const [agregando, setAgregando] = useState(false);
  const [confirmando, setConfirmando] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const esBorrador = recepcion.estado === 'borrador';
  const pendientes = recepcion.items.filter((i) => !i.etiquetado);
  const etiquetados = recepcion.items.length - pendientes.length;

  async function confirmar() {
    setError(null);
    try {
      await api('POST', `/compras/recepciones/${recepcion.id}/confirmar`);
      setConfirmando(false);
      recargar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setConfirmando(false);
    }
  }

  async function etiquetar(itemId: string) {
    try {
      await api('POST', `/compras/recepciones/${recepcion.id}/items/${itemId}/etiquetar`);
      recargar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <>
      <Encabezado
        titulo={recepcion.proveedor_nombre ?? 'Recepción sin proveedor'}
        subtitulo={`Creada el ${fecha(recepcion.creada_en)}${recepcion.observaciones ? ` · ${recepcion.observaciones}` : ''}`}
        accion={
          <div className="flex items-center gap-3">
            <Insignia tono={recepcion.estado === 'borrador' ? 'neutro' : recepcion.estado === 'confirmada' ? 'ambar' : 'verde'}>
              {recepcion.estado}
            </Insignia>
            {esBorrador && recepcion.items.length > 0 && (
              <Boton onClick={() => setConfirmando(true)}>Confirmar recepción</Boton>
            )}
          </div>
        }
      />

      <MensajeError error={error} />

      {!esBorrador && (
        <div className="mb-5 rounded-xl border border-stone-200 bg-white p-5 shadow-sm">
          <div className="mb-2 flex items-center justify-between text-sm">
            <span className="font-medium text-stone-700">Recorrido de etiquetado</span>
            <span className="text-stone-500">{etiquetados} de {recepcion.items.length}</span>
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-stone-100">
            <div
              className="h-full rounded-full bg-acento-500 transition-all"
              style={{ width: `${recepcion.items.length === 0 ? 0 : (etiquetados / recepcion.items.length) * 100}%` }}
            />
          </div>
          {recepcion.estado === 'completada' && (
            <p className="mt-2 text-sm text-acento-700">✓ Todo etiquetado — recepción completada.</p>
          )}
        </div>
      )}

      <Tarjeta
        titulo={`Ítems (${recepcion.items.length})`}
        accion={esBorrador ? <Boton chico onClick={() => setAgregando(true)}>+ Agregar producto</Boton> : undefined}
      >
        {recepcion.items.length === 0 ? (
          <EstadoVacio mensaje="Todavía no cargaste productos. Agregá el primero." />
        ) : (
          <Tabla encabezados={['Producto', 'Cantidad', 'Costo', 'IVA', 'Markup', 'Precio final', 'Vencimiento', esBorrador ? '' : 'Etiqueta']}>
            {recepcion.items.map((item) => (
              <tr key={item.id} className="hover:bg-stone-50">
                <td className="px-3 py-3 font-medium text-stone-800">{item.producto_nombre}</td>
                <td className="px-3 py-3">{fmtCantidad(item.cantidad)}</td>
                <td className="px-3 py-3 text-stone-500">
                  {pesos(item.costo_centavos)}
                  <span className="text-xs text-stone-400"> {item.costo_incluye_iva ? 'c/IVA' : 's/IVA'}</span>
                </td>
                <td className="px-3 py-3 text-stone-500">{parseFloat(item.iva_pct)}%</td>
                <td className="px-3 py-3 text-stone-500">{parseFloat(item.markup_pct)}%</td>
                <td className="px-3 py-3 font-semibold text-stone-800">{pesos(item.precio_final_centavos)}</td>
                <td className="px-3 py-3 text-stone-500">{item.vencimiento ? fecha(item.vencimiento) : '—'}</td>
                <td className="px-3 py-3">
                  {esBorrador ? (
                    <QuitarItem recepcionId={recepcion.id} productoId={item.producto_id} onQuitado={recargar} />
                  ) : item.etiquetado ? (
                    <Insignia tono="verde">✓ etiquetado</Insignia>
                  ) : (
                    <Boton chico variante="secundario" onClick={() => etiquetar(item.id)}>
                      Marcar etiquetado
                    </Boton>
                  )}
                </td>
              </tr>
            ))}
          </Tabla>
        )}
      </Tarjeta>

      {agregando && (
        <ModalItem recepcion={recepcion} onCerrar={() => setAgregando(false)} onAgregado={recargar} />
      )}

      <Modal abierto={confirmando} titulo="Confirmar recepción" onCerrar={() => setConfirmando(false)} ancho="max-w-md">
        <p className="text-sm text-stone-600">
          Al confirmar se actualizan los precios de venta y el stock de {recepcion.items.length} productos,
          y los ítems quedan listos para el recorrido de etiquetado. <strong>Esta acción no se deshace.</strong>
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Boton variante="secundario" onClick={() => setConfirmando(false)}>Todavía no</Boton>
          <Boton onClick={confirmar}>Confirmar</Boton>
        </div>
      </Modal>
    </>
  );
}

function QuitarItem({ recepcionId, productoId, onQuitado }: { recepcionId: string; productoId: string; onQuitado: () => void }) {
  return (
    <Boton chico variante="fantasma" onClick={async () => {
      await api('DELETE', `/compras/recepciones/${recepcionId}/items/${productoId}`);
      onQuitado();
    }}>
      Quitar
    </Boton>
  );
}

// ---------- Alta de ítems en serie: escáner, costo precargado, precio en vivo ----------

function ModalItem({
  recepcion,
  onCerrar,
  onAgregado,
}: {
  recepcion: Detalle;
  onCerrar: () => void;
  /** Refresca la recepción de fondo; el modal queda abierto para seguir cargando. */
  onAgregado: () => void;
}) {
  const [busqueda, setBusqueda] = useState('');
  const [resultados, setResultados] = useState<Producto[]>([]);
  const [producto, setProducto] = useState<Producto | null>(null);
  const [cantidad, setCantidad] = useState('1');
  const [costo, setCosto] = useState('');
  const [incluyeIva, setIncluyeIva] = useState<boolean | null>(null);
  const [vencimiento, setVencimiento] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);
  /** precios_con_iva del proveedor: resuelve el default "según proveedor". */
  const [proveedorConIva, setProveedorConIva] = useState<boolean | null>(null);
  /** Redondeo comercial configurado, para que el precio en vivo coincida. */
  const [redondeo, setRedondeo] = useState(0);
  const temporizador = useRef<number | undefined>(undefined);
  const refBusqueda = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api<ConfiguracionNegocio>('GET', '/catalogo/configuracion')
      .then((c) => setRedondeo(c.redondeo_precio_centavos))
      .catch(() => {});
    if (!recepcion.proveedor_id) return;
    api<Proveedor[]>('GET', '/compras/proveedores?incluir_inactivos=true')
      .then((ps) => setProveedorConIva(ps.find((p) => p.id === recepcion.proveedor_id)?.precios_con_iva ?? null))
      .catch(() => {});
  }, [recepcion.proveedor_id]);

  function seleccionar(p: Producto) {
    setProducto(p);
    setResultados([]);
    setBusqueda('');
    // El último costo conocido viene precargado: solo se corrige si cambió.
    setCosto(desdeCentavos(p.costo_actual_centavos));
    setError(null);
  }

  function buscar(termino: string) {
    setBusqueda(termino);
    window.clearTimeout(temporizador.current);
    if (termino.trim().length < 1) { setResultados([]); return; }
    temporizador.current = window.setTimeout(async () => {
      const r = await api<Producto[]>('GET', `/catalogo/productos?buscar=${encodeURIComponent(termino.trim())}&limite=8`);
      setResultados(r);
    }, 150);
  }

  /** Enter en el buscador: primero código de barras exacto (escáner), después el primer resultado. */
  async function alEnterBusqueda() {
    const termino = busqueda.trim();
    if (!termino) return;
    try {
      const r = await api<{ producto_id: string }>('GET', `/catalogo/codigos-barras/${encodeURIComponent(termino)}`);
      const p = await api<Producto>('GET', `/catalogo/productos/${r.producto_id}`);
      seleccionar(p);
      return;
    } catch (err) {
      if (err instanceof ErrorApi && err.status !== 404) {
        setError(err.message);
        return;
      }
      /* no era un código: cae al primer resultado por nombre */
    }
    if (resultados.length > 0) seleccionar(resultados[0]);
  }

  // Precio de venta en vivo, con la misma regla que el backend:
  // costo → IVA (si no lo incluye) → markup, redondeo solo al final.
  const costoCentavos = aCentavos(costo);
  const incluyeIvaEfectivo = incluyeIva ?? proveedorConIva ?? true;
  const precioVivo =
    producto && costoCentavos !== null && costoCentavos > 0
      ? redondearComercial(
          Math.round(
            costoCentavos *
              (incluyeIvaEfectivo ? 1 : 1 + parseFloat(producto.iva_pct_resuelto) / 100) *
              (1 + parseFloat(producto.markup_pct_resuelto) / 100),
          ),
          redondeo,
        )
      : null;

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    if (!producto || guardando) return;
    if (costoCentavos === null) { setError('Costo inválido'); return; }
    setError(null);
    setGuardando(true);
    try {
      await api('PUT', `/compras/recepciones/${recepcion.id}/items`, {
        producto_id: producto.id,
        cantidad: cantidad.replace(',', '.'),
        costo_centavos: costoCentavos,
        costo_incluye_iva: incluyeIva,
        vencimiento: vencimiento || null,
      });
      onAgregado();
      // Carga continua: se limpia el formulario y se sigue con el próximo.
      setAviso(`✓ ${producto.nombre} agregado`);
      window.setTimeout(() => setAviso(null), 2500);
      setProducto(null);
      setBusqueda('');
      setResultados([]);
      setCantidad('1');
      setCosto('');
      setIncluyeIva(null);
      setVencimiento('');
      window.setTimeout(() => refBusqueda.current?.focus(), 0);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    } finally {
      setGuardando(false);
    }
  }

  return (
    <Modal abierto titulo={`Cargar productos (${recepcion.items.length} en la recepción)`} onCerrar={onCerrar}>
      <form onSubmit={guardar} className="space-y-4">
        {aviso && (
          <p className="rounded-lg border border-acento-200 bg-acento-50 px-3 py-2 text-sm font-medium text-acento-800">
            {aviso}
          </p>
        )}

        {!producto ? (
          <Campo etiqueta="Buscar producto" ayuda="Escaneá el código de barras o escribí el nombre y Enter.">
            <input
              ref={refBusqueda}
              className={claseInput}
              value={busqueda}
              onChange={(e) => buscar(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), alEnterBusqueda())}
              placeholder="Código de barras o nombre…"
              autoFocus
            />
            {resultados.length > 0 && (
              <ul className="mt-2 max-h-56 divide-y divide-stone-100 overflow-y-auto rounded-lg border border-stone-200">
                {resultados.map((p) => (
                  <li key={p.id}>
                    <button type="button"
                      className="flex w-full items-center justify-between px-3 py-2.5 text-left text-sm hover:bg-acento-50"
                      onClick={() => seleccionar(p)}>
                      <span className="font-medium text-stone-800">{p.nombre}</span>
                      <span className="text-xs text-stone-400">
                        costo {pesos(p.costo_actual_centavos)} · IVA {parseFloat(p.iva_pct_resuelto)}% · markup {parseFloat(p.markup_pct_resuelto)}%
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Campo>
        ) : (
          <div className="flex items-center justify-between rounded-lg bg-acento-50 px-4 py-3">
            <div>
              <p className="font-medium text-stone-800">{producto.nombre}</p>
              <p className="text-xs text-stone-500">
                último costo {pesos(producto.costo_actual_centavos)} · precio actual {pesos(producto.precio_actual_centavos)}
                {producto.controla_vencimiento && ' · exige vencimiento'}
              </p>
            </div>
            <Boton chico variante="fantasma" onClick={() => { setProducto(null); window.setTimeout(() => refBusqueda.current?.focus(), 0); }}>
              Cambiar
            </Boton>
          </div>
        )}

        {producto && (
          <>
            <div className="grid grid-cols-2 gap-4">
              <Campo etiqueta={producto.unidad_de_venta === 'peso' ? 'Cantidad (kg)' : 'Cantidad'}>
                <input className={claseInput} value={cantidad} onChange={(e) => setCantidad(e.target.value)} inputMode="decimal" />
              </Campo>
              <Campo etiqueta="Costo unitario ($)" ayuda={costo ? 'Precargado con el último costo' : undefined}>
                <input className={claseInput} value={costo} onChange={(e) => setCosto(e.target.value)}
                  inputMode="decimal" autoFocus onFocus={(e) => e.target.select()} />
              </Campo>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Campo etiqueta="El costo…" ayuda="Por defecto usa la configuración del proveedor">
                <select className={claseInput}
                  value={incluyeIva === null ? '' : incluyeIva ? 'si' : 'no'}
                  onChange={(e) => setIncluyeIva(e.target.value === '' ? null : e.target.value === 'si')}>
                  <option value="">Según proveedor{proveedorConIva !== null ? ` (${proveedorConIva ? 'incluye' : 'no incluye'} IVA)` : ''}</option>
                  <option value="si">Incluye IVA</option>
                  <option value="no">No incluye IVA</option>
                </select>
              </Campo>
              {producto.controla_vencimiento && (
                <Campo etiqueta="Vencimiento (obligatorio)">
                  <input type="date" className={claseInput} value={vencimiento} onChange={(e) => setVencimiento(e.target.value)} />
                </Campo>
              )}
            </div>

            {precioVivo !== null && (
              <div className="flex items-center justify-between rounded-xl bg-stone-50 px-4 py-3">
                <span className="text-sm text-stone-600">
                  Precio de venta resultante
                  {redondeo > 1 && (
                    <span className="block text-xs text-stone-400">redondeado a múltiplos de {pesos(redondeo)}</span>
                  )}
                </span>
                <span className="text-right">
                  <span className="text-xl font-bold tabular-nums text-acento-800">{pesos(precioVivo)}</span>
                  {producto.precio_actual_centavos !== null && producto.precio_actual_centavos !== precioVivo && (
                    <span className="block text-xs text-stone-400">hoy {pesos(producto.precio_actual_centavos)}</span>
                  )}
                </span>
              </div>
            )}
          </>
        )}

        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Listo</Boton>
          <Boton tipo="submit" deshabilitado={!producto || !costo || guardando}>
            {guardando ? 'Agregando…' : 'Agregar y seguir'}
          </Boton>
        </div>
      </form>
    </Modal>
  );
}
