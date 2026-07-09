// Caja offline-first. Con conexión opera contra la API (el límite de fiado
// bloquea con saldo fresco); sin conexión escanea y busca contra el catálogo
// local (IndexedDB) y encola sesión/ventas con UUID propio — el backend es
// idempotente, reintentar jamás duplica. Fiado y cierre de caja: solo online.

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  api, ErrorApi, tienePermiso, usuarioGuardado,
  type Categoria, type Cliente, type MedioPago, type Producto, type SesionCaja, type VentaResumen,
} from '../lib/api';
import { buscarLocal, porCodigoLocal, sincronizarCatalogo } from '../lib/catalogoLocal';
import {
  alCambiarCola, descartarOperacion, encolar, estadoCola, iniciarSincronizacion,
  operacionesConError, type EstadoCola,
} from '../lib/colaSync';
import { useConexion } from '../lib/conexion';
import { borrarMeta, escribirMeta, leerMeta, type ProductoCaja } from '../lib/db';
import { aCentavos, fechaHora, pesos } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tarjeta } from './ui';

interface LineaCarrito {
  /** Identidad de la línea: los ítems personalizados comparten producto_id. */
  clave: string;
  producto: ProductoCaja;
  cantidad: number;
  /** Ítem personalizado (nombre y precio ad hoc): nunca se fusiona. */
  libre?: boolean;
}

/**
 * Pseudo-código de barras del producto genérico que respalda los ítems
 * personalizados (bolsas de maní propias, etc.). El backend guarda el nombre
 * y el precio como snapshot por línea, así que un único producto del
 * catálogo alcanza para todos.
 */
const CODIGO_PERSONALIZADO = 'PERSONALIZADO';

async function productoBasePersonalizado(enLinea: boolean): Promise<ProductoCaja> {
  const local = await porCodigoLocal(CODIGO_PERSONALIZADO);
  if (local) return local;
  if (!enLinea) {
    throw new Error(
      'El producto base "personalizado" todavía no está en el catálogo local; usá esta función una vez con conexión.',
    );
  }
  try {
    const r = await api<{ producto_id: string }>('GET', `/catalogo/codigos-barras/${CODIGO_PERSONALIZADO}`);
    const p = await api<Producto>('GET', `/catalogo/productos/${r.producto_id}`);
    sincronizarCatalogo().catch(() => {});
    return aProductoCaja(p);
  } catch (err) {
    if (!(err instanceof ErrorApi) || err.status !== 404) {
      throw err instanceof Error ? err : new Error('error inesperado');
    }
  }
  // No existe todavía: se crea una única vez y queda en el catálogo.
  if (!tienePermiso('gestionar_catalogo')) {
    throw new Error(
      'Falta el producto base "Producto personalizado" en el catálogo; pedile a un encargado que use esta función una vez.',
    );
  }
  const categorias = await api<Categoria[]>('GET', '/catalogo/categorias');
  if (categorias.length === 0) throw new Error('No hay categorías en el catálogo.');
  const p = await api<Producto>('POST', '/catalogo/productos', {
    nombre: 'Producto personalizado',
    categoria_id: categorias[0].id,
    codigos_barras: [CODIGO_PERSONALIZADO],
  });
  sincronizarCatalogo().catch(() => {});
  return aProductoCaja(p);
}

interface SesionLocal {
  id: string;
  usuario_id: string;
  usuario_nombre: string;
  monto_inicial_centavos: number;
  abierta_en: string;
  /** true si la apertura todavía viaja en la cola (nació offline). */
  local: boolean;
}

const MEDIOS: { valor: MedioPago; etiqueta: string }[] = [
  { valor: 'efectivo', etiqueta: 'Efectivo' },
  { valor: 'tarjeta', etiqueta: 'Tarjeta' },
  { valor: 'mercado_pago', etiqueta: 'Mercado Pago' },
  { valor: 'transferencia', etiqueta: 'Transferencia' },
  { valor: 'cuenta_corriente', etiqueta: 'Cuenta corriente (fiado)' },
];

function aProductoCaja(p: Producto): ProductoCaja {
  return {
    id: p.id,
    nombre: p.nombre,
    unidad_de_venta: p.unidad_de_venta,
    precio_actual_centavos: p.precio_actual_centavos,
    iva_pct: p.iva_pct_resuelto,
    codigos_barras: p.codigos_barras,
  };
}

const esFalloDeRed = (e: unknown) => !(e instanceof ErrorApi);

// ---------- Descuentos preestablecidos (por puesto, en localStorage) ----------

const CLAVE_PRESETS_DESCUENTO = 'pos_descuentos_preset';
const PRESETS_POR_DEFECTO = [5, 10, 15];

function leerPresetsDescuento(): number[] {
  if (typeof localStorage === 'undefined') return PRESETS_POR_DEFECTO;
  try {
    const crudo = localStorage.getItem(CLAVE_PRESETS_DESCUENTO);
    const valores: unknown = crudo ? JSON.parse(crudo) : null;
    if (
      Array.isArray(valores) &&
      valores.length > 0 &&
      valores.every((v) => typeof v === 'number' && Number.isFinite(v) && v > 0 && v <= 100)
    ) {
      return valores as number[];
    }
  } catch { /* preferencia corrupta: se vuelve al defecto */ }
  return PRESETS_POR_DEFECTO;
}

function guardarPresetsDescuento(presets: number[]) {
  localStorage.setItem(CLAVE_PRESETS_DESCUENTO, JSON.stringify(presets));
}

export default function Caja() {
  const enLinea = useConexion();
  const [sesion, setSesion] = useState<SesionLocal | null | 'sin-sesion' | 'cargando'>('cargando');

  const cargarSesion = useCallback(async () => {
    const yo = usuarioGuardado();
    if (!yo) return;
    if (navigator.onLine) {
      try {
        const abiertas = await api<SesionCaja[]>('GET', '/ventas/sesiones?solo_abiertas=true');
        const mia = abiertas.find((s) => s.usuario_id === yo.id);
        if (mia) {
          const local: SesionLocal = {
            id: mia.id,
            usuario_id: mia.usuario_id,
            usuario_nombre: mia.usuario_nombre,
            monto_inicial_centavos: mia.monto_inicial_centavos,
            abierta_en: mia.abierta_en,
            local: false,
          };
          await escribirMeta('sesion_caja', local);
          setSesion(local);
        } else {
          // Puede haber una apertura local aún no sincronizada.
          const pendiente = await leerMeta<SesionLocal>('sesion_caja');
          if (pendiente && pendiente.usuario_id === yo.id && pendiente.local) {
            setSesion(pendiente);
          } else {
            await borrarMeta('sesion_caja');
            setSesion('sin-sesion');
          }
        }
        return;
      } catch {
        /* sin red real: cae al camino offline */
      }
    }
    const guardada = await leerMeta<SesionLocal>('sesion_caja');
    setSesion(guardada && guardada.usuario_id === yo.id ? guardada : 'sin-sesion');
  }, []);

  useEffect(() => {
    iniciarSincronizacion();
    void cargarSesion();
    if (navigator.onLine) sincronizarCatalogo().catch(() => {});
    // Cuando la cola se vacía (p. ej. la apertura local ya se sincronizó),
    // se refresca la sesión desde el servidor y deja de ser "local".
    return alCambiarCola(() => {
      void estadoCola().then((cola) => {
        if (cola.pendientes === 0 && navigator.onLine) void cargarSesion();
      });
    });
  }, [cargarSesion]);

  return (
    <Shell seccion="/caja" amplio>
      {sesion === 'cargando' ? (
        <Cargando />
      ) : sesion === 'sin-sesion' || sesion === null ? (
        <AbrirCaja enLinea={enLinea} onAbierta={cargarSesion} />
      ) : (
        <Venta sesion={sesion} enLinea={enLinea} onSesionCerrada={cargarSesion} />
      )}
    </Shell>
  );
}

// ---------- Indicador de conexión y cola ----------

function IndicadorConexion({ enLinea }: { enLinea: boolean }) {
  const [cola, setCola] = useState<EstadoCola>({ pendientes: 0, con_error: 0 });
  const [verErrores, setVerErrores] = useState(false);

  useEffect(() => {
    const refrescar = () => void estadoCola().then(setCola);
    refrescar();
    return alCambiarCola(refrescar);
  }, []);

  return (
    <>
      <div className="flex items-center gap-2">
        <span className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium ${
          enLinea ? 'bg-acento-100 text-acento-800' : 'bg-amber-100 text-amber-800'
        }`}>
          <span className={`h-1.5 w-1.5 rounded-full ${enLinea ? 'bg-acento-500' : 'bg-amber-500'}`} />
          {enLinea ? 'En línea' : 'Sin conexión'}
        </span>
        {cola.pendientes > 0 && (
          <Insignia tono="ambar">{cola.pendientes} por sincronizar</Insignia>
        )}
        {cola.con_error > 0 && (
          <button onClick={() => setVerErrores(true)}>
            <Insignia tono="rojo">{cola.con_error} con error</Insignia>
          </button>
        )}
      </div>
      {verErrores && <ModalErroresSync onCerrar={() => setVerErrores(false)} />}
    </>
  );
}

function ModalErroresSync({ onCerrar }: { onCerrar: () => void }) {
  const [ops, setOps] = useState<Awaited<ReturnType<typeof operacionesConError>>>([]);

  const cargar = useCallback(() => void operacionesConError().then(setOps), []);
  useEffect(() => cargar(), [cargar]);

  return (
    <Modal abierto titulo="Operaciones rechazadas por el servidor" onCerrar={onCerrar} ancho="max-w-xl">
      {ops.length === 0 ? (
        <EstadoVacio mensaje="No queda nada por revisar." />
      ) : (
        <ul className="divide-y divide-stone-100">
          {ops.map((op) => (
            <li key={op.id} className="flex items-center justify-between gap-3 py-3">
              <div className="min-w-0">
                <p className="text-sm font-medium text-stone-800">{op.descripcion}</p>
                <p className="text-xs text-red-600">{op.error}</p>
                <p className="text-xs text-stone-400">{fechaHora(op.creado_en)}</p>
              </div>
              <Boton chico variante="peligro" onClick={async () => { await descartarOperacion(op.id); cargar(); }}>
                Descartar
              </Boton>
            </li>
          ))}
        </ul>
      )}
      <p className="mt-4 text-xs text-stone-400">
        El servidor rechazó estas operaciones (por ejemplo, un límite de crédito). Revisalas con el
        encargado antes de descartarlas: descartar no revierte nada en el mostrador.
      </p>
    </Modal>
  );
}

// ---------- Apertura ----------

function AbrirCaja({ enLinea, onAbierta }: { enLinea: boolean; onAbierta: () => void }) {
  const [monto, setMonto] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function abrir(e: React.FormEvent) {
    e.preventDefault();
    const centavos = aCentavos(monto || '0');
    if (centavos === null) { setError('Monto inválido'); return; }
    const yo = usuarioGuardado();
    if (!yo) return;
    const id = crypto.randomUUID();
    const cuerpo = { id, monto_inicial_centavos: centavos };

    async function abrirLocal() {
      await escribirMeta('sesion_caja', {
        id,
        usuario_id: yo!.id,
        usuario_nombre: yo!.nombre,
        monto_inicial_centavos: centavos!,
        abierta_en: new Date().toISOString(),
        local: true,
      } satisfies SesionLocal);
      await encolar({
        id,
        descripcion: `Apertura de caja (${pesos(centavos!)})`,
        metodo: 'POST',
        ruta: '/ventas/sesiones',
        cuerpo,
      });
      onAbierta();
    }

    if (!enLinea) {
      await abrirLocal();
      return;
    }
    try {
      await api('POST', '/ventas/sesiones', cuerpo);
      onAbierta();
    } catch (err) {
      if (esFalloDeRed(err)) await abrirLocal();
      else setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <div className="mx-auto mt-16 max-w-sm">
      <div className="mb-4 flex justify-center"><IndicadorConexion enLinea={enLinea} /></div>
      <Tarjeta titulo="Abrir caja">
        <form onSubmit={abrir} className="space-y-4">
          <p className="text-sm text-stone-500">No tenés una sesión abierta. Contá el efectivo inicial del cajón.</p>
          <Campo etiqueta="Monto inicial ($)">
            <input className={claseInput + ' text-lg font-semibold'} value={monto}
              onChange={(e) => setMonto(e.target.value)} inputMode="decimal" autoFocus placeholder="0,00" />
          </Campo>
          <MensajeError error={error} />
          <Boton tipo="submit">Abrir caja</Boton>
          {!enLinea && (
            <p className="text-xs text-amber-600">Sin conexión: la apertura se sincroniza al volver la red.</p>
          )}
        </form>
      </Tarjeta>
    </div>
  );
}

// ---------- Sub-cajas: varios tickets en paralelo dentro de una sesión ----------

/** El "carrito" de un cliente. Una sesión de caja puede tener varios en paralelo. */
interface Ticket {
  id: string;
  lineas: LineaCarrito[];
  descuento: string;
  /** Porcentaje preestablecido activo; se recalcula si cambia el subtotal. */
  descuentoPct: number | null;
  motivoDescuento: string;
  /** Último producto agregado — se muestra en el visor grande. */
  ultimo: { cantidad: number; nombre: string; precio: number | null } | null;
}

function ticketVacio(): Ticket {
  return {
    id: crypto.randomUUID(),
    lineas: [],
    descuento: '',
    descuentoPct: null,
    motivoDescuento: '',
    ultimo: null,
  };
}

/**
 * Arma la venta y decide encolar (offline / sesión local) vs. mandarla directo.
 * La usan tanto el cobro rápido en efectivo como el modal de "otro medio".
 */
async function ejecutarVenta({
  sesion, enLinea, lineas, total, descuentoCentavos, motivoDescuento, pagos, clienteId,
}: {
  sesion: SesionLocal;
  enLinea: boolean;
  lineas: LineaCarrito[];
  total: number;
  descuentoCentavos: number;
  motivoDescuento: string;
  pagos: { medio: MedioPago; monto_centavos: number }[];
  clienteId: string | null;
}): Promise<string> {
  const hayFiado = pagos.some((p) => p.medio === 'cuenta_corriente');
  const cuerpo = {
    id: crypto.randomUUID(),
    sesion_id: sesion.id,
    cliente_id: hayFiado ? clienteId : null,
    total_centavos: total,
    descuento_centavos: descuentoCentavos,
    descuento_motivo: descuentoCentavos > 0 ? motivoDescuento || 'descuento de caja' : null,
    vendida_en: new Date().toISOString(),
    items: lineas.map((l) => ({
      producto_id: l.producto.id,
      producto_nombre: l.producto.nombre,
      precio_unitario_centavos: l.producto.precio_actual_centavos ?? 0,
      cantidad: String(l.cantidad),
      iva_pct: l.producto.iva_pct,
      // El descuento va a nivel ticket; el backend verifica
      // total = Σ subtotales − descuento.
      subtotal_centavos: Math.round((l.producto.precio_actual_centavos ?? 0) * l.cantidad),
    })),
    pagos,
  };

  async function encolarVenta() {
    await encolar({
      id: cuerpo.id,
      descripcion: `Venta ${pesos(total)} (${lineas.length} ítems)`,
      metodo: 'POST',
      ruta: '/ventas',
      cuerpo,
    });
    return 'Venta guardada — se sincroniza al volver la conexión ✓';
  }

  // La venta de una sesión que nació offline debe ir DETRÁS de la
  // apertura en la cola, nunca directo.
  if (!enLinea || sesion.local) return encolarVenta();
  try {
    await api('POST', '/ventas', cuerpo);
    return 'Venta registrada ✓';
  } catch (err) {
    if (esFalloDeRed(err) && !hayFiado) return encolarVenta();
    throw err instanceof Error ? err : new Error('error');
  }
}

// ---------- Venta ----------

function Venta({
  sesion, enLinea, onSesionCerrada,
}: {
  sesion: SesionLocal;
  enLinea: boolean;
  onSesionCerrada: () => void;
}) {
  const [tickets, setTickets] = useState<Ticket[]>(() => [ticketVacio()]);
  const [activoId, setActivoId] = useState(() => tickets[0].id);
  const [busqueda, setBusqueda] = useState('');
  const [resultados, setResultados] = useState<ProductoCaja[]>([]);
  const [dtoAbierto, setDtoAbierto] = useState(false);
  const [presets, setPresets] = useState<number[]>(leerPresetsDescuento);
  const [editandoPresets, setEditandoPresets] = useState(false);
  const [pagando, setPagando] = useState(false);
  const [cobrandoRapido, setCobrandoRapido] = useState(false);
  const [cerrando, setCerrando] = useState(false);
  const [verVentas, setVerVentas] = useState(false);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Código escaneado que no existe en el catálogo: se ofrece cargarlo ya. */
  const [altaRapida, setAltaRapida] = useState<string | null>(null);
  /** Modal de ítem personalizado (nombre y precio ad hoc). */
  const [itemLibre, setItemLibre] = useState(false);
  const temporizador = useRef<number | undefined>(undefined);
  const refBusqueda = useRef<HTMLInputElement>(null);
  const refDescuento = useRef<HTMLInputElement>(null);
  const refLista = useRef<HTMLDivElement>(null);

  const activo = tickets.find((t) => t.id === activoId) ?? tickets[0];
  const lineas = activo.lineas;

  const hayModal =
    pagando || cerrando || verVentas || altaRapida !== null || editandoPresets || itemLibre || cobrandoRapido;
  const puedeCobrar = lineas.length > 0 && !lineas.some((l) => l.cantidad <= 0);
  const puedeCerrarCaja = tienePermiso('cerrar_caja');

  /** Aplica un cambio (parcial o derivado del estado actual) solo al ticket activo. */
  function actualizarActivo(cambio: Partial<Ticket> | ((t: Ticket) => Partial<Ticket>)) {
    setTickets((prev) =>
      prev.map((t) => (t.id === activoId ? { ...t, ...(typeof cambio === 'function' ? cambio(t) : cambio) } : t)),
    );
  }

  function nuevaSubcaja() {
    const t = ticketVacio();
    setTickets((prev) => [...prev, t]);
    setActivoId(t.id);
    setDtoAbierto(false);
    refBusqueda.current?.focus();
  }

  function cerrarSubcaja(id: string) {
    const objetivo = tickets.find((t) => t.id === id);
    if (!objetivo) return;
    if (objetivo.lineas.length > 0 && !window.confirm('Esta sub-caja tiene productos cargados. ¿Cerrarla igual?')) return;
    const restantes = tickets.filter((t) => t.id !== id);
    const nuevos = restantes.length > 0 ? restantes : [ticketVacio()];
    setTickets(nuevos);
    if (id === activoId) setActivoId(nuevos[0].id);
  }

  function cancelarTicket() {
    if (lineas.length === 0) return;
    if (!window.confirm('¿Cancelar el ticket completo?')) return;
    actualizarActivo({ lineas: [], descuento: '', descuentoPct: null, motivoDescuento: '', ultimo: null });
    setDtoAbierto(false);
    refBusqueda.current?.focus();
  }

  // Atajos de teclado del mostrador. Sin lista de dependencias a propósito:
  // el manejador se rearma en cada render y siempre ve el estado fresco.
  useEffect(() => {
    function manejar(e: KeyboardEvent) {
      if (hayModal) return;
      // Ctrl+1..Ctrl+9 saltan directo a esa sub-caja (si existe).
      if (e.ctrlKey && e.key >= '1' && e.key <= '9') {
        e.preventDefault();
        const destino = tickets[Number(e.key) - 1];
        if (destino) setActivoId(destino.id);
        return;
      }
      switch (e.key) {
        case 'F2':
          e.preventDefault();
          refBusqueda.current?.focus();
          refBusqueda.current?.select();
          break;
        case 'F3':
          e.preventDefault();
          setItemLibre(true);
          break;
        case 'F4':
          e.preventDefault();
          abrirDescuento();
          break;
        case 'F6':
          if (enLinea) { e.preventDefault(); setVerVentas(true); }
          break;
        case 'F7':
          e.preventDefault();
          if (puedeCobrar) setPagando(true);
          break;
        case 'F8':
          if (puedeCerrarCaja && enLinea && !sesion.local) { e.preventDefault(); setCerrando(true); }
          break;
        case 'F9':
          e.preventDefault();
          cancelarTicket();
          break;
        case 'F10':
          e.preventDefault();
          if (puedeCobrar) void cobrarEfectivoRapido();
          break;
        default: {
          // El escáner "tipea" donde esté el foco: si ningún campo está
          // activo, el tipeo cae a la búsqueda para que escanear ande siempre.
          const elementoActivo = document.activeElement;
          const esCampo =
            elementoActivo instanceof HTMLInputElement ||
            elementoActivo instanceof HTMLTextAreaElement ||
            elementoActivo instanceof HTMLSelectElement;
          if (!esCampo && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
            refBusqueda.current?.focus();
          }
        }
      }
    }
    window.addEventListener('keydown', manejar);
    return () => window.removeEventListener('keydown', manejar);
  });

  const subtotal = lineas.reduce(
    (suma, l) => suma + Math.round((l.producto.precio_actual_centavos ?? 0) * l.cantidad),
    0,
  );
  // El descuento nunca supera el subtotal (el backend exige
  // total = Σ subtotales − descuento, sin negativos). Con un porcentaje
  // activo se deriva del subtotal vivo: agregar productos lo recalcula.
  const descuentoCentavos = Math.min(
    activo.descuentoPct !== null
      ? Math.round((subtotal * activo.descuentoPct) / 100)
      : aCentavos(activo.descuento || '0') ?? 0,
    subtotal,
  );
  const total = subtotal - descuentoCentavos;

  function aplicarDescuentoPct(pct: number) {
    actualizarActivo({ descuentoPct: pct, descuento: '', motivoDescuento: `descuento ${pct}%` });
  }

  function quitarDescuento() {
    actualizarActivo({ descuentoPct: null, descuento: '', motivoDescuento: '' });
  }

  function abrirDescuento() {
    setDtoAbierto(true);
    // El input recién existe tras el re-render.
    window.setTimeout(() => { refDescuento.current?.focus(); refDescuento.current?.select(); }, 0);
  }

  function agregarProducto(p: ProductoCaja) {
    if (p.precio_actual_centavos === null) {
      setError(`"${p.nombre}" no tiene precio cargado; recibilo o asignale precio antes de venderlo.`);
      return;
    }
    setError(null);
    // Los ítems personalizados nunca se fusionan: comparten producto_id
    // pero cada uno tiene su propio nombre y precio.
    const existente = lineas.find((l) => !l.libre && l.producto.id === p.id);
    const cantidadNueva = existente && p.unidad_de_venta === 'unidad' ? existente.cantidad + 1 : 1;
    actualizarActivo((t) => {
      const ya = t.lineas.find((l) => !l.libre && l.producto.id === p.id);
      const lineasNuevas =
        ya && p.unidad_de_venta === 'unidad'
          ? t.lineas.map((l) => (l.clave === ya.clave ? { ...l, cantidad: l.cantidad + 1 } : l))
          : [...t.lineas, { clave: crypto.randomUUID(), producto: p, cantidad: 1 }];
      return { lineas: lineasNuevas, ultimo: { cantidad: cantidadNueva, nombre: p.nombre, precio: p.precio_actual_centavos } };
    });
    // Un debounce de búsqueda pendiente no debe reabrir el desplegable
    // después de escanear.
    window.clearTimeout(temporizador.current);
    setBusqueda('');
    setResultados([]);
    refBusqueda.current?.focus();
  }

  async function alEnter() {
    const termino = busqueda.trim();
    if (!termino) return;
    // Primero como código de barras exacto (el escáner "tipea" y manda Enter).
    if (enLinea) {
      try {
        const r = await api<{ producto_id: string }>('GET', `/catalogo/codigos-barras/${encodeURIComponent(termino)}`);
        const p = await api<Producto>('GET', `/catalogo/productos/${r.producto_id}`);
        agregarProducto(aProductoCaja(p));
        return;
      } catch (err) {
        if (esFalloDeRed(err)) {
          const local = await porCodigoLocal(termino);
          if (local) { agregarProducto(local); return; }
        }
        /* no era un código: cae a búsqueda por nombre */
      }
    } else {
      const local = await porCodigoLocal(termino);
      if (local) { agregarProducto(local); return; }
    }
    if (resultados.length > 0) { agregarProducto(resultados[0]); return; }
    // Un código escaneado que no existe en el catálogo: se ofrece cargarlo ya.
    // Algunos códigos internos son de 1-3 dígitos, así que no hay piso de longitud.
    if (/^\d+$/.test(termino)) {
      if (!enLinea) {
        setError(`El código ${termino} no está en el catálogo local y sin conexión no se puede crear.`);
      } else if (!tienePermiso('gestionar_catalogo')) {
        setError(`El código ${termino} no está en el catálogo. Pedile a un encargado que lo cargue.`);
      } else {
        setBusqueda('');
        setResultados([]);
        setAltaRapida(termino);
      }
    }
  }

  function alEscribir(valor: string) {
    setBusqueda(valor);
    window.clearTimeout(temporizador.current);
    // Sugerencias desde la primera letra (el backend busca por subcadena).
    if (valor.trim().length < 1) { setResultados([]); return; }
    temporizador.current = window.setTimeout(async () => {
      if (enLinea) {
        try {
          const r = await api<Producto[]>('GET', `/catalogo/productos?buscar=${encodeURIComponent(valor.trim())}&limite=8`);
          setResultados(r.map(aProductoCaja));
          return;
        } catch {
          /* cae al catálogo local */
        }
      }
      setResultados(await buscarLocal(valor, 8));
    }, 150);
  }

  function cambiarCantidad(clave: string, texto: string) {
    const valor = parseFloat(texto.replace(',', '.'));
    actualizarActivo((t) => ({
      lineas: t.lineas.map((l) => (l.clave === clave ? { ...l, cantidad: Number.isFinite(valor) ? valor : 0 } : l)),
    }));
  }

  async function agregarItemLibre(nombre: string, precioCentavos: number) {
    const base = await productoBasePersonalizado(enLinea);
    setError(null);
    actualizarActivo((t) => ({
      lineas: [...t.lineas, {
        clave: crypto.randomUUID(),
        producto: { ...base, nombre, precio_actual_centavos: precioCentavos, codigos_barras: [] },
        cantidad: 1,
        libre: true,
      }],
      ultimo: { cantidad: 1, nombre, precio: precioCentavos },
    }));
    setItemLibre(false);
    refBusqueda.current?.focus();
  }

  // El ticket con scroll siempre muestra lo último que se escaneó (y vuelve
  // arriba al cambiar de sub-caja).
  useEffect(() => {
    refLista.current?.scrollTo({ top: refLista.current.scrollHeight });
  }, [lineas.length, activoId]);

  /**
   * Se llama al confirmar cualquier venta (cobro rápido o modal). La sub-caja
   * recién cobrada se cierra si hay otras abiertas (el cliente ya se fue);
   * si era la única, queda vacía in-place.
   */
  function ventaConfirmada(mensaje: string) {
    const restantes = tickets.filter((t) => t.id !== activoId);
    if (restantes.length > 0) {
      setTickets(restantes);
      setActivoId(restantes[0].id);
    } else {
      const nuevo = ticketVacio();
      setTickets([nuevo]);
      setActivoId(nuevo.id);
    }
    setDtoAbierto(false);
    setPagando(false);
    setAviso(mensaje);
    window.setTimeout(() => setAviso(null), 3000);
    refBusqueda.current?.focus();
  }

  /** F10 / botón "Cobrar": cobra todo en efectivo, sin modal, pago exacto. */
  async function cobrarEfectivoRapido() {
    if (!puedeCobrar || cobrandoRapido) return;
    setError(null);
    setCobrandoRapido(true);
    try {
      const mensaje = await ejecutarVenta({
        sesion,
        enLinea,
        lineas: activo.lineas,
        total,
        descuentoCentavos,
        motivoDescuento: activo.motivoDescuento,
        pagos: [{ medio: 'efectivo', monto_centavos: total }],
        clienteId: null,
      });
      ventaConfirmada(mensaje);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error al cobrar');
    } finally {
      setCobrandoRapido(false);
    }
  }

  return (
    <>
      <Encabezado
        titulo="Caja"
        subtitulo={`Sesión de ${sesion.usuario_nombre} · abierta ${fechaHora(sesion.abierta_en)}`}
        accion={
          <div className="flex items-center gap-3">
            <IndicadorConexion enLinea={enLinea} />
            <Boton variante="secundario" onClick={() => setVerVentas(true)} deshabilitado={!enLinea}>
              Ventas
            </Boton>
            {tienePermiso('cerrar_caja') && (
              <Boton variante="secundario" onClick={() => setCerrando(true)} deshabilitado={!enLinea || sesion.local}>
                Cerrar caja
              </Boton>
            )}
          </div>
        }
      />

      {/* Sub-cajas: varios tickets en paralelo para atender más de un cliente a la vez. */}
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        {tickets.map((t, i) => (
          <div
            key={t.id}
            className={`inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition ${
              t.id === activoId
                ? 'border-acento-600 bg-acento-50 text-acento-800'
                : 'border-stone-300 bg-white text-stone-600 hover:bg-stone-50'
            }`}
          >
            <button
              type="button"
              onClick={() => setActivoId(t.id)}
              disabled={hayModal}
              className="flex items-center gap-1.5 disabled:cursor-not-allowed"
            >
              {i < 9 && (
                <kbd className="rounded bg-stone-800 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-white">
                  Ctrl+{i + 1}
                </kbd>
              )}
              <span>Cliente {i + 1}</span>
              {t.lineas.length > 0 && (
                <span className="rounded-full bg-stone-200 px-1.5 text-[11px] font-semibold tabular-nums text-stone-600">
                  {t.lineas.length}
                </span>
              )}
            </button>
            {tickets.length > 1 && (
              <button
                type="button"
                onClick={() => cerrarSubcaja(t.id)}
                disabled={hayModal}
                aria-label="Cerrar sub-caja"
                className="text-stone-300 hover:text-red-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M18 6 6 18M6 6l12 12" /></svg>
              </button>
            )}
          </div>
        ))}
        <button
          type="button"
          onClick={nuevaSubcaja}
          disabled={hayModal}
          className="inline-flex items-center gap-1 rounded-lg border border-dashed border-stone-300 px-3 py-1.5 text-sm font-medium text-stone-500 transition hover:border-acento-400 hover:text-acento-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          + Nueva sub-caja
        </button>
      </div>

      {aviso && (
        <div className="mb-4 mt-4 rounded-lg border border-acento-200 bg-acento-50 px-4 py-2.5 text-sm font-medium text-acento-800">
          {aviso}
        </div>
      )}
      <MensajeError error={error} />

      {/* Visor grande de mostrador: ítems, total y último producto. */}
      <div className="mt-2 rounded-2xl border border-stone-200 bg-white px-6 py-5 shadow-sm">
        <div className="flex flex-wrap items-end justify-between gap-6">
          <div>
            <p className="text-xs font-semibold uppercase tracking-widest text-stone-400">Ítems</p>
            <p className="text-5xl font-bold leading-none tabular-nums text-stone-700">{lineas.length}</p>
          </div>
          <div className="min-w-0 text-right">
            <p className="text-xs font-semibold uppercase tracking-widest text-stone-400">Importe total</p>
            <p className="truncate text-6xl font-bold leading-none tracking-tight tabular-nums text-stone-900 sm:text-7xl">
              {pesos(total)}
            </p>
          </div>
        </div>
        <p className="mt-4 min-h-[1.75rem] truncate border-t border-stone-100 pt-2.5 text-lg">
          {activo.ultimo ? (
            <>
              <span className="font-semibold text-acento-700">
                {String(activo.ultimo.cantidad).replace('.', ',')} × {pesos(activo.ultimo.precio)}
              </span>
              <span className="ml-2 text-stone-700">{activo.ultimo.nombre}</span>
            </>
          ) : (
            <span className="text-stone-400">Escaneá un producto para empezar</span>
          )}
        </p>
      </div>

      <div className="mt-5 grid gap-5 lg:grid-cols-[1fr_380px]">
        <Tarjeta>
          <div className="relative">
            <input
              ref={refBusqueda}
              className={claseInput + ' py-4 text-lg'}
              placeholder="Escaneá un código o buscá por nombre…  (F2)"
              value={busqueda}
              onChange={(e) => alEscribir(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), alEnter())}
              autoFocus
            />
            {resultados.length > 0 && (
              <ul className="absolute z-10 mt-1 w-full divide-y divide-stone-100 overflow-hidden rounded-lg border border-stone-200 bg-white shadow-lg">
                {resultados.map((p) => (
                  <li key={p.id}>
                    <button type="button"
                      className="flex w-full items-center justify-between px-4 py-3 text-left text-base hover:bg-acento-50"
                      onClick={() => agregarProducto(p)}>
                      <span className="font-medium text-stone-800">{p.nombre}</span>
                      <span className="font-semibold text-stone-600">{pesos(p.precio_actual_centavos)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {lineas.length === 0 ? (
            <EstadoVacio mensaje="El ticket está vacío. Escaneá el primer producto." />
          ) : (
            <div ref={refLista} className="mt-3 max-h-[46vh] overflow-y-auto pr-1">
              <ul className="divide-y divide-stone-100">
                {lineas.map((l) => (
                  <li key={l.clave} className="flex items-center gap-2.5 py-2">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-stone-800">
                        {l.producto.nombre}
                        {l.libre && <span className="ml-1.5 align-middle text-[10px] font-semibold uppercase tracking-wide text-sky-600">personalizado</span>}
                      </p>
                      <p className="text-xs text-stone-400">
                        {pesos(l.producto.precio_actual_centavos)}
                        {l.producto.unidad_de_venta === 'peso' ? ' /kg' : ' c/u'}
                      </p>
                    </div>
                    <input
                      className="w-16 rounded-lg border border-stone-300 px-1.5 py-1 text-center text-sm font-medium focus:border-acento-500 focus:outline-none"
                      value={String(l.cantidad).replace('.', ',')}
                      onChange={(e) => cambiarCantidad(l.clave, e.target.value)}
                      inputMode="decimal"
                    />
                    <p className="w-24 text-right text-sm font-semibold tabular-nums text-stone-800">
                      {pesos(Math.round((l.producto.precio_actual_centavos ?? 0) * l.cantidad))}
                    </p>
                    <button
                      className="rounded-lg p-1.5 text-stone-300 hover:bg-red-50 hover:text-red-500"
                      onClick={() => actualizarActivo((t) => ({ lineas: t.lineas.filter((x) => x.clave !== l.clave) }))}
                      aria-label="Quitar"
                    >
                      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </Tarjeta>

        <div className="space-y-5">
          <Tarjeta titulo="Ticket">
            <dl className="space-y-3 text-base">
              <div className="flex justify-between text-stone-500">
                <dt>Subtotal</dt><dd className="tabular-nums">{pesos(subtotal)}</dd>
              </div>
              <div>
                <button
                  type="button"
                  onClick={() => (dtoAbierto ? setDtoAbierto(false) : abrirDescuento())}
                  className="flex w-full items-center justify-between gap-2 text-stone-500 transition hover:text-stone-700"
                >
                  <span className="inline-flex items-center gap-1.5">
                    Descuento
                    <kbd className="rounded bg-stone-100 px-1 py-0.5 font-mono text-[10px] text-stone-500">F4</kbd>
                    <svg className={`transition-transform ${dtoAbierto ? 'rotate-180' : ''}`}
                      width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <path d="m6 9 6 6 6-6" />
                    </svg>
                  </span>
                  <span className={descuentoCentavos > 0 ? 'font-medium text-red-600' : 'text-stone-400'}>
                    {descuentoCentavos > 0
                      ? `−${pesos(descuentoCentavos)}${activo.descuentoPct !== null ? ` (${activo.descuentoPct}%)` : ''}`
                      : '—'}
                  </span>
                </button>

                {dtoAbierto && (
                  <div className="mt-2.5 space-y-2.5 rounded-xl bg-stone-50 p-3">
                    <div className="flex flex-wrap items-center gap-1.5">
                      {presets.map((pct) => (
                        <button
                          key={pct}
                          type="button"
                          onClick={() => aplicarDescuentoPct(pct)}
                          className={`rounded-full px-3 py-1 text-sm font-semibold transition ${
                            activo.descuentoPct === pct
                              ? 'bg-acento-600 text-white shadow-sm'
                              : 'border border-stone-300 bg-white text-stone-600 hover:bg-stone-100'
                          }`}
                        >
                          {String(pct).replace('.', ',')}%
                        </button>
                      ))}
                      {descuentoCentavos > 0 && (
                        <button
                          type="button"
                          onClick={quitarDescuento}
                          className="rounded-full px-2.5 py-1 text-sm text-stone-400 transition hover:text-red-600"
                        >
                          Quitar
                        </button>
                      )}
                      <span className="flex-1" />
                      <button
                        type="button"
                        onClick={() => setEditandoPresets(true)}
                        aria-label="Configurar porcentajes"
                        title="Configurar porcentajes"
                        className="rounded-lg p-1 text-stone-400 transition hover:bg-stone-200/70 hover:text-stone-600"
                      >
                        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <circle cx="12" cy="12" r="3" />
                          <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.03-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.56-1.03 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.01A1.7 1.7 0 0 0 10 4.09V4a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.01a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.56 1.03z" />
                        </svg>
                      </button>
                    </div>
                    <div className="flex items-center justify-between gap-2 text-sm text-stone-500">
                      <span>Monto fijo ($)</span>
                      <input
                        ref={refDescuento}
                        className="w-28 rounded-lg border border-stone-300 px-2 py-1.5 text-right text-base"
                        value={activo.descuento}
                        onChange={(e) => actualizarActivo({ descuento: e.target.value, descuentoPct: null })}
                        placeholder="0,00"
                        inputMode="decimal"
                      />
                    </div>
                    {descuentoCentavos > 0 && (
                      <input className={claseInput + ' text-sm'} value={activo.motivoDescuento}
                        onChange={(e) => actualizarActivo({ motivoDescuento: e.target.value })} placeholder="Motivo del descuento" />
                    )}
                  </div>
                )}
              </div>
              <div className="flex items-baseline justify-between border-t border-stone-200 pt-3 font-bold text-stone-900">
                <dt className="text-lg">Total</dt>
                <dd className="text-3xl tabular-nums">{pesos(total)}</dd>
              </div>
            </dl>
            <div className="mt-5">
              <Boton grande deshabilitado={!puedeCobrar || cobrandoRapido} onClick={() => void cobrarEfectivoRapido()}>
                {cobrandoRapido ? 'Cobrando…' : `Cobrar ${pesos(total)} en efectivo (F10)`}
              </Boton>
              <button
                type="button"
                onClick={() => setPagando(true)}
                disabled={!puedeCobrar}
                className="mt-2 w-full text-center text-sm font-medium text-stone-500 transition hover:text-acento-700 disabled:cursor-not-allowed disabled:opacity-40"
              >
                Otro medio de pago <kbd className="rounded bg-stone-100 px-1 py-0.5 font-mono text-[10px] text-stone-500">F7</kbd>
              </button>
            </div>
          </Tarjeta>
        </div>
      </div>

      {/* Botonera de atajos, como las cajas registradoras clásicas. */}
      <div className="mt-5 flex flex-wrap gap-2">
        <BotonAtajo tecla="F2" onClick={() => { refBusqueda.current?.focus(); refBusqueda.current?.select(); }}>
          Buscar / escanear
        </BotonAtajo>
        <BotonAtajo tecla="F3" onClick={() => setItemLibre(true)}>
          Ítem personalizado
        </BotonAtajo>
        <BotonAtajo tecla="F4" onClick={abrirDescuento}>
          Descuento
        </BotonAtajo>
        <BotonAtajo tecla="F9" onClick={cancelarTicket} deshabilitado={lineas.length === 0}>
          Cancelar ticket
        </BotonAtajo>
        <BotonAtajo tecla="F10" onClick={() => void cobrarEfectivoRapido()} deshabilitado={!puedeCobrar || cobrandoRapido}>
          Cobrar (efectivo)
        </BotonAtajo>
        <BotonAtajo tecla="F7" onClick={() => setPagando(true)} deshabilitado={!puedeCobrar}>
          Otro medio de pago
        </BotonAtajo>
        <BotonAtajo tecla="F6" onClick={() => setVerVentas(true)} deshabilitado={!enLinea}>
          Ventas
        </BotonAtajo>
        {puedeCerrarCaja && (
          <BotonAtajo tecla="F8" onClick={() => setCerrando(true)} deshabilitado={!enLinea || sesion.local}>
            Cerrar caja
          </BotonAtajo>
        )}
      </div>

      {pagando && (
        <ModalCobro
          sesion={sesion}
          enLinea={enLinea}
          lineas={activo.lineas}
          total={total}
          descuentoCentavos={descuentoCentavos}
          motivoDescuento={activo.motivoDescuento}
          onCerrar={() => setPagando(false)}
          onConfirmada={ventaConfirmada}
        />
      )}
      {cerrando && <ModalCierre sesion={sesion} onCerrar={() => setCerrando(false)} onCerrada={onSesionCerrada} />}
      {verVentas && <ModalVentas sesion={sesion} onCerrar={() => setVerVentas(false)} />}
      {altaRapida !== null && (
        <ModalAltaRapida
          codigo={altaRapida}
          onCerrar={() => { setAltaRapida(null); refBusqueda.current?.focus(); }}
          onCreado={(p) => { setAltaRapida(null); agregarProducto(p); }}
        />
      )}
      {itemLibre && (
        <ModalItemLibre onCerrar={() => setItemLibre(false)} onAgregar={agregarItemLibre} />
      )}
      {editandoPresets && (
        <ModalPresetsDescuento
          actuales={presets}
          onCerrar={() => setEditandoPresets(false)}
          onGuardar={(nuevos) => {
            guardarPresetsDescuento(nuevos);
            setPresets(nuevos);
            setEditandoPresets(false);
          }}
        />
      )}
    </>
  );
}

// ---------- Ítem personalizado (nombre y precio ad hoc) ----------

function ModalItemLibre({
  onCerrar, onAgregar,
}: {
  onCerrar: () => void;
  onAgregar: (nombre: string, precioCentavos: number) => Promise<void>;
}) {
  const [nombre, setNombre] = useState('');
  const [precio, setPrecio] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [agregando, setAgregando] = useState(false);

  async function enviar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (!nombre.trim()) { setError('Poné un nombre descriptivo.'); return; }
    const centavos = aCentavos(precio);
    if (centavos === null || centavos <= 0) { setError('Precio inválido.'); return; }
    setAgregando(true);
    try {
      await onAgregar(nombre.trim(), centavos);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setAgregando(false);
    }
  }

  return (
    <Modal abierto titulo="Ítem personalizado" onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={enviar} className="space-y-4">
        <p className="text-sm text-stone-500">
          Para lo que no tiene código de barras: productos armados acá (bolsas de maní,
          canastas), servicios, etc. El nombre y el precio quedan registrados tal cual en la venta.
        </p>
        <Campo etiqueta="Descripción">
          <input className={claseInput + ' text-base'} value={nombre}
            onChange={(e) => setNombre(e.target.value)} autoFocus placeholder="Bolsa de maní 150 g" />
        </Campo>
        <Campo etiqueta="Precio ($)">
          <input className={claseInput + ' text-lg font-semibold'} value={precio}
            onChange={(e) => setPrecio(e.target.value)} inputMode="decimal" placeholder="0,00" />
        </Campo>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2 pt-1">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={agregando || !nombre.trim()}>
            {agregando ? 'Agregando…' : 'Agregar al ticket'}
          </Boton>
        </div>
      </form>
    </Modal>
  );
}

// ---------- Configuración de los descuentos preestablecidos ----------

function ModalPresetsDescuento({
  actuales, onCerrar, onGuardar,
}: {
  actuales: number[];
  onCerrar: () => void;
  onGuardar: (presets: number[]) => void;
}) {
  const [texto, setTexto] = useState(actuales.join(', '));
  const [error, setError] = useState<string | null>(null);

  function guardar(e: React.FormEvent) {
    e.preventDefault();
    // La coma separa porcentajes; para medios puntos se usa punto: "7.5".
    const valores = texto.split(/[;,]\s*|\s+/).map((t) => t.trim()).filter(Boolean).map(Number);
    if (valores.length === 0 || valores.some((v) => !Number.isFinite(v) || v <= 0 || v > 100)) {
      setError('Ingresá porcentajes entre 0 y 100, separados por coma (decimales con punto: 7.5). Ejemplo: 5, 10, 15');
      return;
    }
    // Sin duplicados y de menor a mayor.
    onGuardar([...new Set(valores)].sort((a, b) => a - b));
  }

  return (
    <Modal abierto titulo="Descuentos preestablecidos" onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={guardar} className="space-y-4">
        <p className="text-sm text-stone-500">
          Estos porcentajes aparecen como botones rápidos en el ticket. Se guardan en este equipo.
        </p>
        <Campo etiqueta="Porcentajes (separados por coma)">
          <input className={claseInput + ' text-base'} value={texto}
            onChange={(e) => setTexto(e.target.value)} autoFocus placeholder="5, 10, 15" />
        </Campo>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit">Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}

function BotonAtajo({
  tecla, children, onClick, deshabilitado = false,
}: {
  tecla: string;
  children: React.ReactNode;
  onClick: () => void;
  deshabilitado?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={deshabilitado}
      className="inline-flex items-center gap-2 rounded-lg border border-stone-300 bg-white px-3.5 py-2.5 text-sm
        font-medium text-stone-700 shadow-sm transition hover:bg-stone-50
        disabled:cursor-not-allowed disabled:opacity-40"
    >
      <kbd className="rounded bg-stone-800 px-1.5 py-0.5 font-mono text-xs font-semibold text-white">{tecla}</kbd>
      {children}
    </button>
  );
}

// ---------- Alta rápida desde el escáner ----------

/**
 * Un código escaneado que no existe: se carga el producto sin salir de la
 * caja. Crea el producto con el código ya asociado, le pone precio (si el
 * operador tiene permiso) y lo agrega directo al ticket.
 */
function ModalAltaRapida({
  codigo, onCerrar, onCreado,
}: {
  codigo: string;
  onCerrar: () => void;
  onCreado: (p: ProductoCaja) => void;
}) {
  const [modo, setModo] = useState<'nuevo' | 'existente'>('nuevo');

  // ---- modo "nuevo": crear un producto y asociarle este código ----
  const [categorias, setCategorias] = useState<Categoria[]>([]);
  const [nombre, setNombre] = useState('');
  const [categoriaId, setCategoriaId] = useState('');
  const [precio, setPrecio] = useState('');
  const puedePrecio = tienePermiso('modificar_precios');

  // ---- modo "existente": el producto ya está cargado, solo falta el código ----
  const [buscar, setBuscar] = useState('');
  const [resultados, setResultados] = useState<Producto[]>([]);
  const [elegido, setElegido] = useState<Producto | null>(null);
  const temporizador = useRef<number | undefined>(undefined);

  const [error, setError] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);

  useEffect(() => {
    api<Categoria[]>('GET', '/catalogo/categorias')
      .then((cs) => {
        setCategorias(cs);
        if (cs.length > 0) setCategoriaId((actual) => actual || cs[0].id);
      })
      .catch(() => setError('No se pudieron cargar las categorías.'));
  }, []);

  function alEscribirExistente(valor: string) {
    setBuscar(valor);
    setElegido(null);
    window.clearTimeout(temporizador.current);
    if (valor.trim().length < 1) { setResultados([]); return; }
    temporizador.current = window.setTimeout(async () => {
      try {
        const r = await api<Producto[]>('GET', `/catalogo/productos?buscar=${encodeURIComponent(valor.trim())}&limite=8`);
        setResultados(r);
      } catch {
        setResultados([]);
      }
    }, 200);
  }

  async function guardarNuevo(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const centavos = puedePrecio ? aCentavos(precio) : null;
    if (puedePrecio && centavos === null) { setError('Precio inválido'); return; }
    setGuardando(true);
    try {
      const p = await api<Producto>('POST', '/catalogo/productos', {
        nombre: nombre.trim(),
        categoria_id: categoriaId,
        codigos_barras: [codigo],
      });
      if (centavos !== null) {
        await api('POST', `/catalogo/productos/${p.id}/precio`, { precio_centavos: centavos });
      }
      sincronizarCatalogo().catch(() => {});
      onCreado({ ...aProductoCaja(p), precio_actual_centavos: centavos });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setGuardando(false);
    }
  }

  async function asociarExistente(e: React.FormEvent) {
    e.preventDefault();
    if (!elegido) return;
    setError(null);
    setGuardando(true);
    try {
      await api('POST', `/catalogo/productos/${elegido.id}/codigos-barras`, { codigo });
      sincronizarCatalogo().catch(() => {});
      onCreado(aProductoCaja({ ...elegido, codigos_barras: [...elegido.codigos_barras, codigo] }));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setGuardando(false);
    }
  }

  return (
    <Modal abierto titulo="Producto no encontrado" onCerrar={onCerrar}>
      <div className="space-y-4">
        <p className="text-sm text-stone-500">
          El código <strong className="font-mono text-stone-800">{codigo}</strong> no está en el catálogo.
        </p>

        <div className="grid grid-cols-2 rounded-lg bg-stone-100 p-1 text-sm font-medium">
          {(['nuevo', 'existente'] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => setModo(m)}
              className={`rounded-md py-1.5 transition ${
                modo === m ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
              }`}
            >
              {m === 'nuevo' ? 'Producto nuevo' : 'Ya existe en el catálogo'}
            </button>
          ))}
        </div>

        {modo === 'nuevo' ? (
          <form onSubmit={guardarNuevo} className="space-y-4">
            <p className="text-sm text-stone-500">
              ¿Lo cargás ahora? Queda asociado al código y va directo al ticket.
            </p>
            <Campo etiqueta="Nombre del producto">
              <input className={claseInput + ' text-base'} value={nombre}
                onChange={(e) => setNombre(e.target.value)} autoFocus />
            </Campo>
            <div className="grid grid-cols-2 gap-4">
              <Campo etiqueta="Categoría">
                <select className={claseInput} value={categoriaId} onChange={(e) => setCategoriaId(e.target.value)}>
                  {categorias.map((c) => (
                    <option key={c.id} value={c.id}>{c.nombre}</option>
                  ))}
                </select>
              </Campo>
              {puedePrecio ? (
                <Campo etiqueta="Precio de venta ($)">
                  <input className={claseInput + ' text-base font-semibold'} value={precio}
                    onChange={(e) => setPrecio(e.target.value)} inputMode="decimal" placeholder="0,00" />
                </Campo>
              ) : (
                <p className="self-end pb-2 text-xs text-amber-600">
                  No tenés permiso para poner precio: el producto se crea sin precio y no se puede vender hasta tenerlo.
                </p>
              )}
            </div>
            <MensajeError error={error} />
            <div className="flex justify-end gap-2 pt-1">
              <Boton variante="secundario" onClick={onCerrar}>No, volver</Boton>
              <Boton tipo="submit" deshabilitado={guardando || !nombre.trim() || !categoriaId}>
                {guardando ? 'Cargando…' : 'Cargar y agregar al ticket'}
              </Boton>
            </div>
          </form>
        ) : (
          <form onSubmit={asociarExistente} className="space-y-4">
            <p className="text-sm text-stone-500">
              Buscá el producto que ya está cargado: le agregamos este código sin duplicarlo.
            </p>
            <div className="relative">
              <Campo etiqueta="Buscar producto">
                <input className={claseInput + ' text-base'} value={buscar}
                  onChange={(e) => alEscribirExistente(e.target.value)} autoFocus placeholder="Nombre del producto…" />
              </Campo>
              {resultados.length > 0 && (
                <ul className="absolute z-10 mt-1 w-full divide-y divide-stone-100 overflow-hidden rounded-lg border border-stone-200 bg-white shadow-lg">
                  {resultados.map((p) => (
                    <li key={p.id}>
                      <button type="button"
                        className="flex w-full items-center justify-between px-4 py-2.5 text-left text-sm hover:bg-acento-50"
                        onClick={() => { setElegido(p); setBuscar(p.nombre); setResultados([]); }}>
                        <span className="font-medium text-stone-800">{p.nombre}</span>
                        <span className="text-stone-500">{pesos(p.precio_actual_centavos)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
            {elegido && (
              <p className="rounded-lg bg-acento-50 px-3 py-2.5 text-sm text-acento-800">
                Se asocia el código a <strong>{elegido.nombre}</strong> y va directo al ticket.
              </p>
            )}
            <MensajeError error={error} />
            <div className="flex justify-end gap-2 pt-1">
              <Boton variante="secundario" onClick={onCerrar}>No, volver</Boton>
              <Boton tipo="submit" deshabilitado={guardando || !elegido}>
                {guardando ? 'Asociando…' : 'Asociar código y agregar al ticket'}
              </Boton>
            </div>
          </form>
        )}
      </div>
    </Modal>
  );
}

// ---------- Cobro ----------

interface FilaPago {
  medio: MedioPago;
  monto: string;
}

function ModalCobro({
  sesion, enLinea, lineas, total, descuentoCentavos, motivoDescuento, onCerrar, onConfirmada,
}: {
  sesion: SesionLocal;
  enLinea: boolean;
  lineas: LineaCarrito[];
  total: number;
  descuentoCentavos: number;
  motivoDescuento: string;
  onCerrar: () => void;
  onConfirmada: (mensaje: string) => void;
}) {
  // El efectivo tiene su propio atajo de cobro rápido (F10, sin modal); este
  // modal es para elegir otro medio, así que arranca en tarjeta.
  const [pagos, setPagos] = useState<FilaPago[]>([{ medio: 'tarjeta', monto: (total / 100).toFixed(2).replace('.', ',') }]);
  const [recibido, setRecibido] = useState('');
  const [clientes, setClientes] = useState<Cliente[]>([]);
  const [clienteId, setClienteId] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [confirmando, setConfirmando] = useState(false);

  const hayFiado = pagos.some((p) => p.medio === 'cuenta_corriente');
  const sumaPagos = pagos.reduce((s, p) => s + (aCentavos(p.monto) ?? 0), 0);
  const soloEfectivo = pagos.length === 1 && pagos[0].medio === 'efectivo';
  const recibidoCentavos = aCentavos(recibido);
  const vuelto = soloEfectivo && recibidoCentavos !== null ? recibidoCentavos - total : null;

  useEffect(() => {
    if (hayFiado && enLinea) api<Cliente[]>('GET', '/clientes?limite=200').then(setClientes).catch(() => {});
  }, [hayFiado, enLinea]);

  function cambiarFila(indice: number, cambio: Partial<FilaPago>) {
    setPagos((prev) => prev.map((p, i) => (i === indice ? { ...p, ...cambio } : p)));
  }

  async function confirmar() {
    if (confirmando) return;
    setError(null);
    if (sumaPagos !== total) {
      setError(`Los pagos suman ${pesos(sumaPagos)} pero el total es ${pesos(total)}.`);
      return;
    }
    if (hayFiado && !enLinea) {
      setError('El fiado necesita conexión: el límite de crédito se controla con el saldo fresco.');
      return;
    }
    if (hayFiado && !clienteId) {
      setError('El fiado necesita un cliente identificado.');
      return;
    }
    setConfirmando(true);
    try {
      const mensaje = await ejecutarVenta({
        sesion,
        enLinea,
        lineas,
        total,
        descuentoCentavos,
        motivoDescuento,
        pagos: pagos.map((p) => ({ medio: p.medio, monto_centavos: aCentavos(p.monto) ?? 0 })),
        clienteId: hayFiado ? clienteId : null,
      });
      onConfirmada(mensaje);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
      setConfirmando(false);
    }
  }

  // F10 confirma también acá: escanear → F10 → F10 cierra la venta sin mouse.
  // Sin lista de dependencias a propósito: el manejador siempre ve estado fresco.
  useEffect(() => {
    function manejar(e: KeyboardEvent) {
      if (e.key === 'F10') {
        e.preventDefault();
        void confirmar();
      }
    }
    window.addEventListener('keydown', manejar);
    return () => window.removeEventListener('keydown', manejar);
  });

  return (
    <Modal abierto titulo={`Cobrar ${pesos(total)}`} onCerrar={onCerrar}>
      <div className="space-y-4">
        {pagos.map((pago, i) => (
          <div key={i} className="flex items-center gap-2">
            <select className={claseInput + ' flex-1'} value={pago.medio}
              onChange={(e) => cambiarFila(i, { medio: e.target.value as MedioPago })}>
              {MEDIOS.map((m) => (
                <option key={m.valor} value={m.valor} disabled={m.valor === 'cuenta_corriente' && !enLinea}>
                  {m.etiqueta}{m.valor === 'cuenta_corriente' && !enLinea ? ' — requiere conexión' : ''}
                </option>
              ))}
            </select>
            <input className={claseInput + ' w-32 text-right font-semibold'} value={pago.monto}
              onChange={(e) => cambiarFila(i, { monto: e.target.value })} inputMode="decimal" />
            {pagos.length > 1 && (
              <button className="p-1 text-stone-300 hover:text-red-500"
                onClick={() => setPagos((prev) => prev.filter((_, x) => x !== i))} aria-label="Quitar pago">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
              </button>
            )}
          </div>
        ))}

        <button className="text-sm font-medium text-acento-700 hover:underline"
          onClick={() => setPagos((prev) => [...prev, { medio: 'tarjeta', monto: '' }])}>
          + Dividir el pago
        </button>

        {hayFiado && enLinea && (
          <Campo etiqueta="Cliente (obligatorio para fiado)">
            <select className={claseInput} value={clienteId} onChange={(e) => setClienteId(e.target.value)}>
              <option value="">— Elegir cliente —</option>
              {clientes.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.nombre} (debe {pesos(c.saldo_actual_centavos)})
                </option>
              ))}
            </select>
          </Campo>
        )}

        {soloEfectivo && (
          <div className="rounded-xl bg-stone-50 p-4">
            <Campo etiqueta="¿Con cuánto paga?">
              <input className={claseInput + ' text-lg font-semibold'} value={recibido}
                onChange={(e) => setRecibido(e.target.value)} inputMode="decimal" placeholder={(total / 100).toFixed(2).replace('.', ',')} />
            </Campo>
            {vuelto !== null && vuelto >= 0 && (
              <p className="mt-3 text-center text-2xl font-bold text-acento-700">Vuelto: {pesos(vuelto)}</p>
            )}
            {vuelto !== null && vuelto < 0 && (
              <p className="mt-3 text-center text-sm font-medium text-red-600">Falta {pesos(-vuelto)}</p>
            )}
          </div>
        )}

        {sumaPagos !== total && (
          <p className="text-sm text-amber-600">
            Pagos: {pesos(sumaPagos)} · Total: {pesos(total)} — tienen que coincidir.
          </p>
        )}
        <MensajeError error={error} />
        <div className="flex justify-end gap-2 pt-1">
          <Boton variante="secundario" onClick={onCerrar}>Volver</Boton>
          <Boton onClick={confirmar} deshabilitado={confirmando || sumaPagos !== total}>
            {confirmando ? 'Registrando…' : 'Confirmar venta (F10)'}
          </Boton>
        </div>
      </div>
    </Modal>
  );
}

// ---------- Cierre con arqueo (solo online) ----------

function ModalCierre({ sesion, onCerrar, onCerrada }: { sesion: SesionLocal; onCerrar: () => void; onCerrada: () => void }) {
  const [detalle, setDetalle] = useState<{ totales_por_medio: { medio: MedioPago; total_centavos: number }[] } | null>(null);
  const [contado, setContado] = useState('');
  const [resultado, setResultado] = useState<{ diferencia: number; esperado: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<typeof detalle>('GET', `/ventas/sesiones/${sesion.id}`).then(setDetalle).catch(() => {});
  }, [sesion.id]);

  const efectivoVentas = detalle?.totales_por_medio.find((t) => t.medio === 'efectivo')?.total_centavos ?? 0;

  async function cerrar(e: React.FormEvent) {
    e.preventDefault();
    const centavos = aCentavos(contado);
    if (centavos === null) { setError('Monto inválido'); return; }
    try {
      const r = await api<{ diferencia_arqueo_centavos: number; efectivo_esperado_centavos: number }>(
        'POST', `/ventas/sesiones/${sesion.id}/cerrar`, { monto_contado_centavos: centavos },
      );
      await borrarMeta('sesion_caja');
      setResultado({ diferencia: r.diferencia_arqueo_centavos, esperado: r.efectivo_esperado_centavos });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo="Cerrar caja" onCerrar={resultado ? onCerrada : onCerrar} ancho="max-w-md">
      {resultado ? (
        <div className="space-y-4 text-center">
          <p className="text-sm text-stone-500">Esperado {pesos(resultado.esperado)} · Contado {pesos(aCentavos(contado) ?? 0)}</p>
          <p className={`text-3xl font-bold ${resultado.diferencia === 0 ? 'text-acento-700' : 'text-red-600'}`}>
            {resultado.diferencia === 0
              ? 'Arqueo perfecto ✓'
              : `${resultado.diferencia > 0 ? 'Sobrante' : 'Faltante'}: ${pesos(Math.abs(resultado.diferencia))}`}
          </p>
          <p className="text-xs text-stone-400">La diferencia queda registrada tal cual — nunca se corrige sola.</p>
          <Boton onClick={onCerrada}>Listo</Boton>
        </div>
      ) : (
        <form onSubmit={cerrar} className="space-y-4">
          <dl className="space-y-1.5 rounded-xl bg-stone-50 p-4 text-sm">
            <div className="flex justify-between text-stone-500">
              <dt>Monto inicial</dt><dd>{pesos(sesion.monto_inicial_centavos)}</dd>
            </div>
            {detalle?.totales_por_medio.map((t) => (
              <div key={t.medio} className="flex justify-between text-stone-500">
                <dt className="capitalize">{t.medio.replace('_', ' ')}</dt><dd>{pesos(t.total_centavos)}</dd>
              </div>
            ))}
            <div className="flex justify-between border-t border-stone-200 pt-2 font-semibold text-stone-800">
              <dt>Efectivo esperado en cajón</dt>
              <dd>{pesos(sesion.monto_inicial_centavos + efectivoVentas)}</dd>
            </div>
          </dl>
          <Campo etiqueta="Efectivo contado ($)">
            <input className={claseInput + ' text-lg font-semibold'} value={contado}
              onChange={(e) => setContado(e.target.value)} inputMode="decimal" autoFocus />
          </Campo>
          <MensajeError error={error} />
          <div className="flex justify-end gap-2">
            <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
            <Boton tipo="submit">Cerrar caja</Boton>
          </div>
        </form>
      )}
    </Modal>
  );
}

// ---------- Ventas de la sesión (solo online) ----------

function ModalVentas({ sesion, onCerrar }: { sesion: SesionLocal; onCerrar: () => void }) {
  const [ventas, setVentas] = useState<VentaResumen[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cargar = useCallback(() => {
    api<VentaResumen[]>('GET', `/ventas?sesion_id=${sesion.id}`).then(setVentas).catch(() => setVentas([]));
  }, [sesion.id]);
  useEffect(() => cargar(), [cargar]);

  async function anular(id: string) {
    try {
      await api('POST', `/ventas/${id}/anular`, { motivo: 'anulada desde caja' });
      cargar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo="Ventas de la sesión" onCerrar={onCerrar} ancho="max-w-2xl">
      <MensajeError error={error} />
      {ventas === null ? (
        <Cargando />
      ) : ventas.length === 0 ? (
        <EstadoVacio mensaje="Todavía no hay ventas sincronizadas en esta sesión." />
      ) : (
        <ul className="max-h-96 divide-y divide-stone-100 overflow-y-auto">
          {ventas.map((v) => (
            <li key={v.id} className="flex items-center justify-between gap-3 py-3">
              <div>
                <p className="font-semibold text-stone-800">{pesos(v.total_centavos)}</p>
                <p className="text-xs text-stone-400">{fechaHora(v.vendida_en)}</p>
              </div>
              <div className="flex items-center gap-2">
                {v.estado === 'anulada' ? (
                  <Insignia tono="rojo">anulada</Insignia>
                ) : (
                  tienePermiso('anular_venta') && (
                    <Boton chico variante="fantasma" onClick={() => anular(v.id)}>Anular</Boton>
                  )
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </Modal>
  );
}
