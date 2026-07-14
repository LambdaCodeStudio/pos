// Catálogo: productos con búsqueda tolerante a typos (pg_trgm en el back),
// alta/edición, códigos de barras, cambio manual de precio; y categorías.

import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { api, tienePermiso, type Categoria, type ConfiguracionNegocio, type Producto } from '../lib/api';
import { aCentavos, desdeCentavos, pesos, redondearComercial } from '../lib/formato';
import { impresoraVinculada, vincularImpresora } from '../lib/impresoraTicket';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

export default function Productos() {
  const [pestana, setPestana] = useState<'productos' | 'categorias'>('productos');
  return (
    <Shell seccion="/productos">
      <Encabezado
        titulo="Catálogo"
        subtitulo="Productos, precios y códigos de barras."
        accion={
          <div className="grid grid-cols-2 rounded-lg bg-stone-200/70 p-1 text-sm font-medium">
            {(['productos', 'categorias'] as const).map((p) => (
              <button
                key={p}
                onClick={() => setPestana(p)}
                className={`rounded-md px-4 py-1.5 capitalize transition ${
                  pestana === p ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
                }`}
              >
                {p === 'categorias' ? 'Categorías' : 'Productos'}
              </button>
            ))}
          </div>
        }
      />
      {pestana === 'productos' ? <TablaProductos /> : <TablaCategorias />}
    </Shell>
  );
}

// ---------- Productos ----------

function TablaProductos() {
  const [productos, setProductos] = useState<Producto[] | null>(null);
  const [buscar, setBuscar] = useState('');
  const [editando, setEditando] = useState<Producto | 'nuevo' | null>(null);
  const [precioDe, setPrecioDe] = useState<Producto | null>(null);
  const [eliminando, setEliminando] = useState<Producto | null>(null);
  const [configurandoRedondeo, setConfigurandoRedondeo] = useState(false);
  const [configurandoTicket, setConfigurandoTicket] = useState(false);
  const temporizador = useRef<number | undefined>(undefined);

  const cargar = useCallback((termino: string) => {
    const q = termino.trim() ? `?buscar=${encodeURIComponent(termino.trim())}&limite=50` : '?limite=50';
    api<Producto[]>('GET', `/catalogo/productos${q}`).then(setProductos).catch(() => setProductos([]));
  }, []);

  useEffect(() => cargar(''), [cargar]);

  function alEscribir(valor: string) {
    setBuscar(valor);
    window.clearTimeout(temporizador.current);
    temporizador.current = window.setTimeout(() => cargar(valor), 250);
  }

  const puedeGestionar = tienePermiso('gestionar_catalogo');
  const puedePrecios = tienePermiso('modificar_precios');

  return (
    <Tarjeta>
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <input
          className={claseInput + ' max-w-sm'}
          placeholder="Buscar por nombre… (tolera errores de tipeo)"
          value={buscar}
          onChange={(e) => alEscribir(e.target.value)}
        />
        <div className="flex-1" />
        {puedePrecios && (
          <Boton variante="secundario" onClick={() => setConfigurandoRedondeo(true)}>
            Redondeo de precios
          </Boton>
        )}
        {puedePrecios && (
          <Boton variante="secundario" onClick={() => setConfigurandoTicket(true)}>
            Configuración del ticket
          </Boton>
        )}
        {puedeGestionar && <Boton onClick={() => setEditando('nuevo')}>+ Nuevo producto</Boton>}
      </div>

      {productos === null ? (
        <Cargando />
      ) : productos.length === 0 ? (
        <EstadoVacio mensaje="Sin resultados." />
      ) : (
        <Tabla encabezados={['Producto', 'Categoría', 'Precio', 'Costo', 'IVA', 'Markup', '', '']}>
          {productos.map((p) => (
            <tr key={p.id} className="group hover:bg-stone-50">
              <td className="px-3 py-3">
                <p className="font-medium text-stone-800">{p.nombre}</p>
                <p className="text-xs text-stone-400">
                  {p.codigos_barras.length > 0 ? p.codigos_barras.join(' · ') : 'sin código'}
                  {p.controla_vencimiento && ' · controla vencimiento'}
                  {p.unidad_de_venta === 'peso' && ' · por peso'}
                </p>
              </td>
              <td className="px-3 py-3 text-stone-500">{p.categoria_nombre}</td>
              <td className="px-3 py-3 font-semibold text-stone-800">{pesos(p.precio_actual_centavos)}</td>
              <td className="px-3 py-3 text-stone-500">{pesos(p.costo_actual_centavos)}</td>
              <td className="px-3 py-3 text-stone-500">{parseFloat(p.iva_pct_resuelto)}%</td>
              <td className="px-3 py-3 text-stone-500">
                {parseFloat(p.markup_pct_resuelto)}%
                {p.markup_pct_override !== null && <span className="text-acento-600"> *</span>}
              </td>
              <td className="px-3 py-3">{!p.activo && <Insignia tono="rojo">inactivo</Insignia>}</td>
              <td className="px-3 py-3 text-right">
                <span className="flex justify-end gap-1">
                  {puedePrecios && (
                    <Boton chico variante="fantasma" onClick={() => setPrecioDe(p)}>Precio</Boton>
                  )}
                  {puedeGestionar && (
                    <Boton chico variante="fantasma" onClick={() => setEditando(p)}>Editar</Boton>
                  )}
                  {puedeGestionar && p.activo && (
                    <Boton chico variante="peligro" onClick={() => setEliminando(p)}>Eliminar</Boton>
                  )}
                </span>
              </td>
            </tr>
          ))}
        </Tabla>
      )}

      {editando && (
        <ModalProducto
          producto={editando === 'nuevo' ? null : editando}
          onCerrar={() => setEditando(null)}
          onGuardado={() => { setEditando(null); cargar(buscar); }}
        />
      )}
      {precioDe && (
        <ModalPrecio
          producto={precioDe}
          onCerrar={() => setPrecioDe(null)}
          onGuardado={() => { setPrecioDe(null); cargar(buscar); }}
        />
      )}
      {configurandoRedondeo && <ModalRedondeo onCerrar={() => setConfigurandoRedondeo(false)} />}
      {configurandoTicket && <ModalConfiguracionTicket onCerrar={() => setConfigurandoTicket(false)} />}
      {eliminando && (
        <ModalEliminarProducto
          producto={eliminando}
          onCerrar={() => setEliminando(null)}
          onEliminado={() => { setEliminando(null); cargar(buscar); }}
        />
      )}
    </Tarjeta>
  );
}

function ModalEliminarProducto({
  producto,
  onCerrar,
  onEliminado,
}: {
  producto: Producto;
  onCerrar: () => void;
  onEliminado: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [eliminando, setEliminando] = useState(false);

  async function confirmar() {
    setError(null);
    setEliminando(true);
    try {
      await api('DELETE', `/catalogo/productos/${producto.id}`);
      onEliminado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudo eliminar el producto.');
      setEliminando(false);
    }
  }

  return (
    <Modal abierto titulo="Eliminar producto" onCerrar={onCerrar} ancho="max-w-sm">
      <div className="space-y-4">
        <p className="text-sm text-stone-600">
          ¿Eliminar <strong className="text-stone-800">{producto.nombre}</strong>? Deja de listarse y
          venderse; su historial de precios y ventas se conserva.
        </p>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar} deshabilitado={eliminando}>Cancelar</Boton>
          <Boton variante="peligro" onClick={confirmar} deshabilitado={eliminando}>
            {eliminando ? 'Eliminando…' : 'Eliminar'}
          </Boton>
        </div>
      </div>
    </Modal>
  );
}

// ---------- Redondeo comercial de precios ----------

const OPCIONES_REDONDEO = [0, 1_000, 5_000, 10_000, 50_000];
const EJEMPLO_CENTAVOS = 463_000; // $4.630, el caso típico del mostrador

function ModalRedondeo({ onCerrar }: { onCerrar: () => void }) {
  const [valor, setValor] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);

  useEffect(() => {
    api<ConfiguracionNegocio>('GET', '/catalogo/configuracion')
      .then((c) => setValor(c.redondeo_precio_centavos))
      .catch(() => setError('No se pudo leer la configuración.'));
  }, []);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    if (valor === null) return;
    setError(null);
    setGuardando(true);
    try {
      await api('PUT', '/catalogo/configuracion', { redondeo_precio_centavos: valor });
      onCerrar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setGuardando(false);
    }
  }

  return (
    <Modal abierto titulo="Redondeo de precios" onCerrar={onCerrar} ancho="max-w-sm">
      {valor === null ? (
        <Cargando />
      ) : (
        <form onSubmit={guardar} className="space-y-4">
          <p className="text-sm text-stone-500">
            El precio de venta calculado en las recepciones se redondea al múltiplo más
            cercano (la mitad sube). Aplica a los precios que se calculen de acá en adelante;
            no toca los precios vigentes.
          </p>
          <div className="space-y-1.5">
            {OPCIONES_REDONDEO.map((opcion) => (
              <label key={opcion}
                className={`flex cursor-pointer items-center justify-between rounded-lg border px-3.5 py-2.5 text-sm transition ${
                  valor === opcion ? 'border-acento-500 bg-acento-50' : 'border-stone-200 hover:bg-stone-50'
                }`}>
                <span className="flex items-center gap-2.5 font-medium text-stone-800">
                  <input type="radio" name="redondeo" checked={valor === opcion} onChange={() => setValor(opcion)}
                    className="h-4 w-4 border-stone-300 text-acento-600 focus:ring-acento-500/30" />
                  {opcion === 0 ? 'Sin redondeo' : `A ${pesos(opcion)}`}
                </span>
                <span className="text-xs text-stone-400">
                  {pesos(EJEMPLO_CENTAVOS)} → {pesos(redondearComercial(EJEMPLO_CENTAVOS, opcion))}
                </span>
              </label>
            ))}
          </div>
          <MensajeError error={error} />
          <div className="flex justify-end gap-2">
            <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
            <Boton tipo="submit" deshabilitado={guardando}>{guardando ? 'Guardando…' : 'Guardar'}</Boton>
          </div>
        </form>
      )}
    </Modal>
  );
}

// ---------- Configuración del ticket impreso ----------

function ModalConfiguracionTicket({ onCerrar }: { onCerrar: () => void }) {
  const [config, setConfig] = useState<ConfiguracionNegocio | null>(null);
  const [encabezado, setEncabezado] = useState('');
  const [pie, setPie] = useState('');
  const [vinculada, setVinculada] = useState(false);
  const [vinculando, setVinculando] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);

  useEffect(() => {
    api<ConfiguracionNegocio>('GET', '/catalogo/configuracion')
      .then((c) => { setConfig(c); setEncabezado(c.ticket_encabezado); setPie(c.ticket_pie); })
      .catch(() => setError('No se pudo leer la configuración.'));
    impresoraVinculada().then(setVinculada).catch(() => {});
  }, []);

  async function vincular() {
    setError(null);
    setVinculando(true);
    try {
      await vincularImpresora();
      setVinculada(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error al vincular la impresora');
    } finally {
      setVinculando(false);
    }
  }

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    if (config === null) return;
    setError(null);
    setGuardando(true);
    try {
      await api('PUT', '/catalogo/configuracion', {
        redondeo_precio_centavos: config.redondeo_precio_centavos,
        ticket_encabezado: encabezado,
        ticket_pie: pie,
      });
      onCerrar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setGuardando(false);
    }
  }

  return (
    <Modal abierto titulo="Configuración del ticket" onCerrar={onCerrar} ancho="max-w-md">
      {config === null ? (
        <Cargando />
      ) : (
        <form onSubmit={guardar} className="space-y-4">
          <Campo etiqueta="Encabezado (nombre del local, dirección, etc.)">
            <textarea
              className={claseInput + ' min-h-20'}
              value={encabezado}
              onChange={(e) => setEncabezado(e.target.value)}
              placeholder={'Kiosco Don José\nAv. Siempre Viva 123'}
            />
          </Campo>
          <Campo etiqueta="Pie (mensaje de despedida, redes sociales, etc.)">
            <textarea
              className={claseInput + ' min-h-16'}
              value={pie}
              onChange={(e) => setPie(e.target.value)}
              placeholder="¡Gracias por su compra!"
            />
          </Campo>
          <div className="space-y-2.5 rounded-lg border border-stone-200 bg-stone-50 px-3.5 py-3">
            <p className="text-sm text-stone-500">
              La impresora se vincula por dispositivo (cada PC del mostrador la vincula una
              vez, desde Chrome o Edge con conexión USB directa).
            </p>
            <Boton
              tipo="button"
              variante="secundario"
              deshabilitado={vinculando}
              onClick={() => void vincular()}
            >
              {vinculando ? 'Vinculando…' : vinculada ? 'Impresora vinculada ✓ — cambiar' : 'Vincular impresora'}
            </Boton>
          </div>
          <MensajeError error={error} />
          <div className="flex justify-end gap-2">
            <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
            <Boton tipo="submit" deshabilitado={guardando}>{guardando ? 'Guardando…' : 'Guardar'}</Boton>
          </div>
        </form>
      )}
    </Modal>
  );
}

export function ModalProducto({
  producto,
  nombreInicial,
  codigoInicial,
  onCerrar,
  onGuardado,
  extra,
}: {
  producto: Producto | null;
  /** Precarga desde otras pantallas (p. ej. "crear producto nuevo" en recepciones). */
  nombreInicial?: string;
  codigoInicial?: string;
  onCerrar: () => void;
  /** En alta, recibe el producto recién creado para que quien abrió el modal pueda usarlo. */
  onGuardado: (producto?: Producto) => void;
  /** Contenido adicional al pie del formulario (p. ej. un link a un flujo alternativo). */
  extra?: ReactNode;
}) {
  const [categorias, setCategorias] = useState<Categoria[]>([]);
  const [nombre, setNombre] = useState(producto?.nombre ?? nombreInicial ?? '');
  const [categoriaId, setCategoriaId] = useState(producto?.categoria_id ?? '');
  const [markup, setMarkup] = useState(producto?.markup_pct_override ?? '');
  const [iva, setIva] = useState(producto?.iva_pct_override ?? '');
  const [unidad, setUnidad] = useState(producto?.unidad_de_venta ?? 'unidad');
  const [controlaVto, setControlaVto] = useState(producto?.controla_vencimiento ?? false);
  const [codigos, setCodigos] = useState(producto?.codigos_barras.join('\n') ?? codigoInicial ?? '');
  const [activo, setActivo] = useState(producto?.activo ?? true);
  const [precioBruto, setPrecioBruto] = useState('');
  const [redondeo, setRedondeo] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);
  const puedePrecio = tienePermiso('modificar_precios');

  // Markup/IVA efectivos para calcular el precio: el override del formulario
  // si está cargado, si no el default de la categoría elegida.
  const categoriaSeleccionada = categorias.find((c) => c.id === categoriaId);
  const ivaEfectivo = iva.trim() !== ''
    ? parseFloat(String(iva).replace(',', '.'))
    : parseFloat(categoriaSeleccionada?.iva_pct ?? '0');
  const markupEfectivo = markup.trim() !== ''
    ? parseFloat(String(markup).replace(',', '.'))
    : parseFloat(categoriaSeleccionada?.markup_pct ?? '0');

  // Mismo cálculo que el backend en recepciones (compras/precio.rs):
  // base = bruto × (1 + iva/100); final = base × (1 + markup/100), con el
  // mismo redondeo comercial configurado en "Redondeo de precios".
  const brutoCentavos = aCentavos(precioBruto);
  const precioConIvaCentavos = brutoCentavos !== null && !Number.isNaN(ivaEfectivo)
    ? Math.round(brutoCentavos * (1 + ivaEfectivo / 100))
    : null;
  const precioFinalCentavos = brutoCentavos !== null && !Number.isNaN(ivaEfectivo) && !Number.isNaN(markupEfectivo)
    ? redondearComercial(Math.round(brutoCentavos * (1 + ivaEfectivo / 100) * (1 + markupEfectivo / 100)), redondeo)
    : null;

  useEffect(() => {
    api<Categoria[]>('GET', '/catalogo/categorias').then((cs) => {
      setCategorias(cs);
      if (!producto && cs.length > 0) setCategoriaId((actual) => actual || cs[0].id);
    });
    if (!producto) {
      api<ConfiguracionNegocio>('GET', '/catalogo/configuracion')
        .then((c) => setRedondeo(c.redondeo_precio_centavos))
        .catch(() => {});
    }
  }, [producto]);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (puedePrecio && precioBruto.trim() !== '' && brutoCentavos === null) {
      setError('Precio bruto inválido');
      return;
    }
    const centavos = puedePrecio && precioBruto.trim() !== '' ? precioFinalCentavos : null;
    setGuardando(true);
    try {
      if (producto) {
        await api('PATCH', `/catalogo/productos/${producto.id}`, {
          nombre,
          categoria_id: categoriaId,
          markup_pct_override: markup.trim() === '' ? null : markup.replace(',', '.'),
          iva_pct_override: iva.trim() === '' ? null : iva.replace(',', '.'),
          unidad_de_venta: unidad,
          controla_vencimiento: controlaVto,
          activo,
        });
        // Códigos: alta de los nuevos (la quita se hace de a uno si hace falta).
        const nuevos = codigos.split('\n').map((c) => c.trim()).filter(Boolean)
          .filter((c) => !producto.codigos_barras.includes(c));
        for (const codigo of nuevos) {
          await api('POST', `/catalogo/productos/${producto.id}/codigos-barras`, { codigo });
        }
      } else {
        const p = await api<Producto>('POST', '/catalogo/productos', {
          nombre,
          categoria_id: categoriaId,
          markup_pct_override: markup.trim() === '' ? null : markup.replace(',', '.'),
          iva_pct_override: iva.trim() === '' ? null : iva.replace(',', '.'),
          unidad_de_venta: unidad,
          controla_vencimiento: controlaVto,
          codigos_barras: codigos.split('\n').map((c) => c.trim()).filter(Boolean),
        });
        if (centavos !== null) {
          await api('POST', `/catalogo/productos/${p.id}/precio`, { precio_centavos: centavos });
        }
        onGuardado(centavos !== null ? { ...p, precio_actual_centavos: centavos } : p);
        return;
      }
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setGuardando(false);
    }
  }

  return (
    <Modal abierto titulo={producto ? 'Editar producto' : 'Nuevo producto'} onCerrar={onCerrar}>
      <form onSubmit={guardar} className="space-y-4">
        <Campo etiqueta="Nombre">
          <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
        </Campo>
        <Campo etiqueta="Categoría">
          <select className={claseInput} value={categoriaId} onChange={(e) => setCategoriaId(e.target.value)}>
            {categorias.map((c) => (
              <option key={c.id} value={c.id}>
                {c.nombre} (markup {parseFloat(c.markup_pct)}% · IVA {parseFloat(c.iva_pct)}%)
              </option>
            ))}
          </select>
        </Campo>
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Markup % (vacío = hereda)" ayuda="Solo de la categoría directa">
            <input className={claseInput} value={markup ?? ''} onChange={(e) => setMarkup(e.target.value)} placeholder="—" />
          </Campo>
          <Campo etiqueta="IVA % (vacío = hereda)" ayuda="21 · 10,5 · 0 exento">
            <input className={claseInput} value={iva ?? ''} onChange={(e) => setIva(e.target.value)} placeholder="—" />
          </Campo>
        </div>
        {!producto && puedePrecio && (
          <div className="grid grid-cols-3 gap-4">
            <Campo etiqueta="Precio bruto ($)" ayuda="Sin IVA ni utilidad">
              <input className={claseInput + ' text-base font-semibold'} value={precioBruto}
                onChange={(e) => setPrecioBruto(e.target.value)} inputMode="decimal" placeholder="0,00" />
            </Campo>
            <Campo etiqueta={`Con IVA (${Number.isFinite(ivaEfectivo) ? ivaEfectivo : 0}%)`}>
              <p className={claseInput + ' bg-stone-50 text-stone-500'}>{pesos(precioConIvaCentavos)}</p>
            </Campo>
            <Campo etiqueta={`Precio final (markup ${Number.isFinite(markupEfectivo) ? markupEfectivo : 0}%)`}
              ayuda={redondeo > 1 ? `Redondeado a ${pesos(redondeo)}` : undefined}>
              <p className={claseInput + ' bg-stone-50 font-semibold text-stone-800'}>{pesos(precioFinalCentavos)}</p>
            </Campo>
          </div>
        )}
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Unidad de venta">
            <select className={claseInput} value={unidad} onChange={(e) => setUnidad(e.target.value as 'unidad' | 'peso')}>
              <option value="unidad">Por unidad</option>
              <option value="peso">Por peso</option>
            </select>
          </Campo>
          <div className="flex flex-col justify-end gap-2 pb-1">
            <label className="flex items-center gap-2 text-sm text-stone-700">
              <input type="checkbox" checked={controlaVto} onChange={(e) => setControlaVto(e.target.checked)}
                className="h-4 w-4 rounded border-stone-300 text-acento-600 focus:ring-acento-500/30" />
              Controla vencimiento
            </label>
            {producto && (
              <label className="flex items-center gap-2 text-sm text-stone-700">
                <input type="checkbox" checked={activo} onChange={(e) => setActivo(e.target.checked)}
                  className="h-4 w-4 rounded border-stone-300 text-acento-600 focus:ring-acento-500/30" />
                Activo
              </label>
            )}
          </div>
        </div>
        <Campo etiqueta="Códigos de barras (uno por línea)">
          <textarea className={claseInput} rows={2} value={codigos} onChange={(e) => setCodigos(e.target.value)} />
        </Campo>
        {extra}
        <MensajeError error={error} />
        <div className="flex justify-end gap-2 pt-1">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={guardando || !nombre.trim() || !categoriaId}>
            {guardando ? 'Guardando…' : 'Guardar'}
          </Boton>
        </div>
      </form>
    </Modal>
  );
}

function ModalPrecio({
  producto,
  onCerrar,
  onGuardado,
}: {
  producto: Producto;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [precio, setPrecio] = useState(desdeCentavos(producto.precio_actual_centavos));
  const [error, setError] = useState<string | null>(null);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    const centavos = aCentavos(precio);
    if (centavos === null) {
      setError('Precio inválido');
      return;
    }
    try {
      await api('POST', `/catalogo/productos/${producto.id}/precio`, { precio_centavos: centavos });
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={`Precio de ${producto.nombre}`} onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={guardar} className="space-y-4">
        <p className="text-sm text-stone-500">
          Precio actual: <strong className="text-stone-800">{pesos(producto.precio_actual_centavos)}</strong>.
          El cambio queda en el historial con tu usuario.
        </p>
        <Campo etiqueta="Nuevo precio ($)">
          <input className={claseInput + ' text-lg font-semibold'} value={precio}
            onChange={(e) => setPrecio(e.target.value)} autoFocus inputMode="decimal" />
        </Campo>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit">Cambiar precio</Boton>
        </div>
      </form>
    </Modal>
  );
}

// ---------- Categorías ----------

function TablaCategorias() {
  const [categorias, setCategorias] = useState<Categoria[] | null>(null);
  const [editando, setEditando] = useState<Categoria | 'nueva' | null>(null);

  const cargar = useCallback(() => {
    api<Categoria[]>('GET', '/catalogo/categorias').then(setCategorias).catch(() => setCategorias([]));
  }, []);
  useEffect(() => cargar(), [cargar]);

  const puede = tienePermiso('gestionar_catalogo');

  return (
    <Tarjeta>
      <div className="mb-4 flex justify-end">
        {puede && <Boton onClick={() => setEditando('nueva')}>+ Nueva categoría</Boton>}
      </div>
      {categorias === null ? (
        <Cargando />
      ) : (
        <Tabla encabezados={['Categoría', 'Markup', 'IVA', '']}>
          {categorias.map((c) => (
            <tr key={c.id} className="group hover:bg-stone-50">
              <td className="px-3 py-3 font-medium text-stone-800">{c.nombre}</td>
              <td className="px-3 py-3 text-stone-500">{parseFloat(c.markup_pct)}%</td>
              <td className="px-3 py-3 text-stone-500">{parseFloat(c.iva_pct)}%</td>
              <td className="px-3 py-3 text-right">
                {puede && (
                  <span className="invisible group-hover:visible">
                    <Boton chico variante="fantasma" onClick={() => setEditando(c)}>Editar</Boton>
                  </span>
                )}
              </td>
            </tr>
          ))}
        </Tabla>
      )}
      {editando && (
        <ModalCategoria
          categoria={editando === 'nueva' ? null : editando}
          onCerrar={() => setEditando(null)}
          onGuardado={() => { setEditando(null); cargar(); }}
        />
      )}
    </Tarjeta>
  );
}

function ModalCategoria({
  categoria,
  onCerrar,
  onGuardado,
}: {
  categoria: Categoria | null;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [nombre, setNombre] = useState(categoria?.nombre ?? '');
  const [markup, setMarkup] = useState(categoria ? String(parseFloat(categoria.markup_pct)) : '40');
  const [iva, setIva] = useState(categoria ? String(parseFloat(categoria.iva_pct)) : '21');
  const [error, setError] = useState<string | null>(null);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      const cuerpo = {
        nombre,
        markup_pct: markup.replace(',', '.'),
        iva_pct: iva.replace(',', '.'),
      };
      if (categoria) await api('PATCH', `/catalogo/categorias/${categoria.id}`, cuerpo);
      else await api('POST', '/catalogo/categorias', cuerpo);
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={categoria ? 'Editar categoría' : 'Nueva categoría'} onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={guardar} className="space-y-4">
        <Campo etiqueta="Nombre">
          <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
        </Campo>
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Markup %">
            <input className={claseInput} value={markup} onChange={(e) => setMarkup(e.target.value)} />
          </Campo>
          <Campo etiqueta="IVA %">
            <input className={claseInput} value={iva} onChange={(e) => setIva(e.target.value)} />
          </Campo>
        </div>
        <p className="text-xs text-stone-400">
          Los productos sin override heredan estos valores — solo de su categoría directa, nunca del árbol.
        </p>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={!nombre.trim()}>Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}
