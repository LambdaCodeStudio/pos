// Auditoría del sistema: línea de tiempo de mutaciones de datos maestros y
// acciones de seguridad, con el diff antes/después desplegado campo por
// campo. Los hechos de negocio (ventas, stock, precios, fiado) viven en sus
// ledgers — esto cubre lo demás: quién tocó qué y cuándo.

import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { fecha as fmtFecha } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, Tarjeta } from './ui';

interface Evento {
  id: string;
  entidad: string;
  entidad_id: string | null;
  entidad_nombre: string | null;
  accion: string;
  usuario_id: string | null;
  usuario_nombre: string | null;
  diff: unknown;
  creado_en: string;
}

interface Respuesta {
  eventos: Evento[];
  hay_mas: boolean;
}

const ENTIDADES = [
  ['', 'Todas las entidades'],
  ['producto', 'Productos'],
  ['categoria', 'Categorías'],
  ['proveedor', 'Proveedores'],
  ['cliente', 'Clientes'],
  ['usuario', 'Usuarios'],
  ['rol', 'Roles'],
] as const;

const ACCIONES = [
  ['', 'Todas las acciones'],
  ['crear', 'Alta'],
  ['actualizar', 'Edición'],
  ['desactivar', 'Desactivación'],
  ['agregar_codigo_barras', 'Alta de código de barras'],
  ['quitar_codigo_barras', 'Baja de código de barras'],
  ['login_fallido', 'Login fallido'],
  ['bootstrap_admin', 'Creación del admin inicial'],
] as const;

const ETIQUETA_ENTIDAD: Record<string, string> = {
  producto: 'el producto',
  categoria: 'la categoría',
  proveedor: 'el proveedor',
  cliente: 'el cliente',
  usuario: 'el usuario',
  rol: 'el rol',
};

const ETIQUETA_CAMPO: Record<string, string> = {
  nombre: 'Nombre',
  padre_id: 'Categoría padre',
  markup_pct: 'Markup %',
  iva_pct: 'IVA %',
  markup_pct_override: 'Markup propio %',
  iva_pct_override: 'IVA propio %',
  controla_vencimiento: 'Controla vencimiento',
  activo: 'Activo',
  categoria_id: 'Categoría',
  codigos_barras: 'Códigos de barras',
  codigo: 'Código',
  cuit: 'CUIT',
  telefono: 'Teléfono',
  documento: 'Documento',
  precios_con_iva: 'Precios con IVA',
  condiciones_pago: 'Condiciones de pago',
  limite_credito_centavos: 'Límite de crédito (centavos)',
  rol_id: 'Rol',
  permisos: 'Permisos',
  permisos_extra: 'Permisos extra',
  tiene_pin: 'Tiene PIN',
  password_cambiada: 'Contraseña cambiada',
  pin_cambiado: 'PIN cambiado',
  descripcion: 'Descripción',
  nombre_intentado: 'Usuario intentado',
};

function describir(e: Evento): { verbo: string; tono: 'verde' | 'azul' | 'rojo' | 'ambar' | 'neutro' } {
  switch (e.accion) {
    case 'crear': return { verbo: 'creó', tono: 'verde' };
    case 'actualizar': return { verbo: 'editó', tono: 'azul' };
    case 'desactivar': return { verbo: 'desactivó', tono: 'rojo' };
    case 'agregar_codigo_barras': return { verbo: 'agregó un código de barras a', tono: 'azul' };
    case 'quitar_codigo_barras': return { verbo: 'quitó un código de barras de', tono: 'ambar' };
    case 'login_fallido': return { verbo: 'intento de acceso fallido', tono: 'rojo' };
    case 'bootstrap_admin': return { verbo: 'creación inicial del administrador', tono: 'neutro' };
    default: return { verbo: e.accion, tono: 'neutro' };
  }
}

function mostrarValor(v: unknown): string {
  if (v === null || v === undefined) return '—';
  if (typeof v === 'boolean') return v ? 'sí' : 'no';
  if (Array.isArray(v)) return v.length === 0 ? '—' : v.map(String).join(', ');
  if (typeof v === 'object') return JSON.stringify(v);
  return String(v);
}

export default function Auditoria() {
  const [entidad, setEntidad] = useState('');
  const [accion, setAccion] = useState('');
  const [desde, setDesde] = useState('');
  const [hasta, setHasta] = useState('');
  const [eventos, setEventos] = useState<Evento[] | null>(null);
  const [hayMas, setHayMas] = useState(false);
  const [cargandoMas, setCargandoMas] = useState(false);

  const consulta = useCallback((offset: number) => {
    const partes = [`limite=50`, `offset=${offset}`];
    if (entidad) partes.push(`entidad=${entidad}`);
    if (accion) partes.push(`accion=${accion}`);
    if (desde) partes.push(`desde=${desde}`);
    if (hasta) partes.push(`hasta=${hasta}`);
    return api<Respuesta>('GET', `/auditoria/eventos?${partes.join('&')}`);
  }, [entidad, accion, desde, hasta]);

  useEffect(() => {
    setEventos(null);
    consulta(0).then((r) => { setEventos(r.eventos); setHayMas(r.hay_mas); }).catch(() => setEventos([]));
  }, [consulta]);

  async function cargarMas() {
    if (!eventos) return;
    setCargandoMas(true);
    try {
      const r = await consulta(eventos.length);
      setEventos([...eventos, ...r.eventos]);
      setHayMas(r.hay_mas);
    } finally {
      setCargandoMas(false);
    }
  }

  // Agrupar por día calendario local.
  const grupos: { dia: string; eventos: Evento[] }[] = [];
  for (const e of eventos ?? []) {
    const dia = new Date(e.creado_en).toLocaleDateString('es-AR', {
      weekday: 'long', day: 'numeric', month: 'long', year: 'numeric',
    });
    const ultimo = grupos[grupos.length - 1];
    if (ultimo && ultimo.dia === dia) ultimo.eventos.push(e);
    else grupos.push({ dia, eventos: [e] });
  }

  return (
    <Shell seccion="/auditoria">
      <Encabezado
        titulo="Auditoría"
        subtitulo="Quién tocó qué y cuándo: datos maestros y acciones de seguridad. Los hechos de negocio están en sus ledgers."
      />

      <Tarjeta>
        <div className="mb-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <select className={claseInput} value={entidad} onChange={(e) => setEntidad(e.target.value)}>
            {ENTIDADES.map(([v, etiqueta]) => <option key={v} value={v}>{etiqueta}</option>)}
          </select>
          <select className={claseInput} value={accion} onChange={(e) => setAccion(e.target.value)}>
            {ACCIONES.map(([v, etiqueta]) => <option key={v} value={v}>{etiqueta}</option>)}
          </select>
          <Campo etiqueta="">
            <input type="date" className={claseInput} value={desde} onChange={(e) => setDesde(e.target.value)} />
          </Campo>
          <Campo etiqueta="">
            <input type="date" className={claseInput} value={hasta} onChange={(e) => setHasta(e.target.value)} />
          </Campo>
        </div>

        {eventos === null ? (
          <Cargando />
        ) : eventos.length === 0 ? (
          <EstadoVacio mensaje="No hay eventos con esos filtros." />
        ) : (
          <div className="space-y-6">
            {grupos.map((grupo) => (
              <section key={grupo.dia}>
                <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-stone-400">
                  {grupo.dia}
                </h3>
                <ol className="relative ml-2 space-y-1 border-l border-stone-200 pl-5">
                  {grupo.eventos.map((e) => <FilaEvento key={e.id} evento={e} />)}
                </ol>
              </section>
            ))}
            {hayMas && (
              <div className="flex justify-center pt-2">
                <Boton variante="secundario" onClick={cargarMas} deshabilitado={cargandoMas}>
                  {cargandoMas ? 'Cargando…' : 'Cargar más'}
                </Boton>
              </div>
            )}
          </div>
        )}
      </Tarjeta>
    </Shell>
  );
}

function FilaEvento({ evento }: { evento: Evento }) {
  const [abierto, setAbierto] = useState(false);
  const { verbo, tono } = describir(evento);
  const hora = new Date(evento.creado_en).toLocaleTimeString('es-AR', { hour: '2-digit', minute: '2-digit' });
  const puntoTono = {
    verde: 'bg-acento-500', azul: 'bg-sky-500', rojo: 'bg-red-500',
    ambar: 'bg-amber-500', neutro: 'bg-stone-400',
  }[tono];
  const esSeguridad = evento.accion === 'login_fallido' || evento.accion === 'bootstrap_admin';
  const tieneDetalle = evento.diff !== null && evento.diff !== undefined;

  return (
    <li className="relative">
      <span className={`absolute -left-[26px] top-2.5 h-2.5 w-2.5 rounded-full ring-4 ring-white ${puntoTono}`} />
      <button
        className={`w-full rounded-lg px-3 py-2 text-left transition hover:bg-stone-50 ${abierto ? 'bg-stone-50' : ''}`}
        onClick={() => tieneDetalle && setAbierto(!abierto)}
      >
        <div className="flex items-baseline justify-between gap-3">
          <p className="text-sm text-stone-700">
            {esSeguridad ? (
              <><Insignia tono={tono}>{verbo}</Insignia>{' '}
                {evento.entidad_nombre && <strong className="font-semibold">{evento.entidad_nombre}</strong>}</>
            ) : (
              <>
                <strong className="font-semibold">{evento.usuario_nombre ?? 'Sistema'}</strong>{' '}
                {verbo} {ETIQUETA_ENTIDAD[evento.entidad] ?? evento.entidad}{' '}
                <strong className="font-semibold">
                  {evento.entidad_nombre ?? (evento.entidad_id ? `${evento.entidad_id.slice(0, 8)}…` : '')}
                </strong>
              </>
            )}
          </p>
          <span className="flex shrink-0 items-center gap-2 text-xs text-stone-400">
            {hora}
            {tieneDetalle && (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
                className={`transition-transform ${abierto ? 'rotate-180' : ''}`}>
                <path d="m6 9 6 6 6-6" />
              </svg>
            )}
          </span>
        </div>
        {abierto && tieneDetalle && <Detalle diff={evento.diff} />}
      </button>
    </li>
  );
}

function Detalle({ diff }: { diff: unknown }) {
  const d = diff as Record<string, unknown>;

  // Diff de edición: {antes: {...}, despues: {...}} → tabla de cambios.
  if (d && typeof d === 'object' && 'antes' in d && 'despues' in d) {
    const antes = (d.antes ?? {}) as Record<string, unknown>;
    const despues = (d.despues ?? {}) as Record<string, unknown>;
    const claves = [...new Set([...Object.keys(antes), ...Object.keys(despues)])];
    const cambios = claves.filter((k) => {
      const nuevo = despues[k];
      // En los PATCH, null/undefined en "despues" significa "no se tocó".
      if (nuevo === null || nuevo === undefined) return false;
      return JSON.stringify(antes[k]) !== JSON.stringify(nuevo);
    });

    return (
      <div className="mt-2 overflow-x-auto rounded-lg border border-stone-200 bg-white">
        {cambios.length === 0 ? (
          <p className="px-3 py-2 text-xs text-stone-400">Sin cambios efectivos (edición vacía o valores iguales).</p>
        ) : (
          <table className="w-full text-xs">
            <tbody className="divide-y divide-stone-100">
              {cambios.map((k) => (
                <tr key={k}>
                  <td className="w-40 px-3 py-2 font-medium text-stone-500">{ETIQUETA_CAMPO[k] ?? k}</td>
                  <td className="px-3 py-2 text-red-600 line-through decoration-red-300">
                    {mostrarValor(antes[k])}
                  </td>
                  <td className="w-6 text-center text-stone-300">→</td>
                  <td className="px-3 py-2 font-medium text-acento-700">{mostrarValor(despues[k])}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    );
  }

  // Payload plano (altas, códigos, login fallido): clave → valor.
  if (d && typeof d === 'object') {
    return (
      <div className="mt-2 overflow-x-auto rounded-lg border border-stone-200 bg-white">
        <table className="w-full text-xs">
          <tbody className="divide-y divide-stone-100">
            {Object.entries(d).map(([k, v]) => (
              <tr key={k}>
                <td className="w-40 px-3 py-2 font-medium text-stone-500">{ETIQUETA_CAMPO[k] ?? k}</td>
                <td className="px-3 py-2 text-stone-700">{mostrarValor(v)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  return <pre className="mt-2 rounded-lg bg-stone-100 p-3 text-xs">{JSON.stringify(diff, null, 2)}</pre>;
}
