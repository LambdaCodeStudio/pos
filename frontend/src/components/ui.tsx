// Kit de UI mínimo y consistente: botones, tarjetas, campos, modal, tabla.

import { type ReactNode, useEffect } from 'react';

export function Boton({
  children,
  onClick,
  variante = 'primario',
  tipo = 'button',
  deshabilitado = false,
  chico = false,
  grande = false,
}: {
  children: ReactNode;
  onClick?: () => void;
  variante?: 'primario' | 'secundario' | 'peligro' | 'fantasma';
  tipo?: 'button' | 'submit';
  deshabilitado?: boolean;
  chico?: boolean;
  grande?: boolean;
}) {
  const base = `inline-flex items-center justify-center gap-2 rounded-lg font-medium transition
    focus:outline-none focus-visible:ring-2 focus-visible:ring-acento-500/50
    disabled:opacity-50 disabled:cursor-not-allowed
    ${chico ? 'px-3 py-1.5 text-sm' : grande ? 'px-6 py-3.5 text-lg' : 'px-4 py-2.5 text-sm'}`;
  const variantes = {
    primario: 'bg-acento-600 text-white hover:bg-acento-700 shadow-sm',
    secundario: 'bg-white text-stone-700 border border-stone-300 hover:bg-stone-50 shadow-sm',
    peligro: 'bg-red-600 text-white hover:bg-red-700 shadow-sm',
    fantasma: 'text-stone-600 hover:bg-stone-200/60',
  };
  return (
    <button type={tipo} onClick={onClick} disabled={deshabilitado} className={`${base} ${variantes[variante]}`}>
      {children}
    </button>
  );
}

export function Tarjeta({ children, titulo, accion }: { children: ReactNode; titulo?: string; accion?: ReactNode }) {
  return (
    <section className="rounded-xl border border-stone-200 bg-white shadow-sm">
      {(titulo || accion) && (
        <header className="flex items-center justify-between border-b border-stone-100 px-5 py-4">
          {titulo && <h2 className="text-sm font-semibold tracking-wide text-stone-700">{titulo}</h2>}
          {accion}
        </header>
      )}
      <div className="p-5">{children}</div>
    </section>
  );
}

export function Insignia({
  children,
  tono = 'neutro',
}: {
  children: ReactNode;
  tono?: 'neutro' | 'verde' | 'ambar' | 'rojo' | 'azul';
}) {
  const tonos = {
    neutro: 'bg-stone-100 text-stone-600',
    verde: 'bg-acento-100 text-acento-800',
    ambar: 'bg-amber-100 text-amber-800',
    rojo: 'bg-red-100 text-red-700',
    azul: 'bg-sky-100 text-sky-800',
  };
  return (
    <span className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${tonos[tono]}`}>
      {children}
    </span>
  );
}

export function Campo({
  etiqueta,
  children,
  ayuda,
}: {
  etiqueta: string;
  children: ReactNode;
  ayuda?: string;
}) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-sm font-medium text-stone-600">{etiqueta}</span>
      {children}
      {ayuda && <span className="mt-1 block text-xs text-stone-400">{ayuda}</span>}
    </label>
  );
}

export const claseInput = `w-full rounded-lg border border-stone-300 bg-white px-3 py-2 text-sm
  text-stone-800 placeholder:text-stone-400 shadow-sm transition
  focus:border-acento-500 focus:outline-none focus:ring-2 focus:ring-acento-500/20`;

export function Modal({
  abierto,
  titulo,
  onCerrar,
  children,
  ancho = 'max-w-lg',
}: {
  abierto: boolean;
  titulo: string;
  onCerrar: () => void;
  children: ReactNode;
  ancho?: string;
}) {
  useEffect(() => {
    if (!abierto) return;
    const manejar = (e: KeyboardEvent) => e.key === 'Escape' && onCerrar();
    window.addEventListener('keydown', manejar);
    return () => window.removeEventListener('keydown', manejar);
  }, [abierto, onCerrar]);

  if (!abierto) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-stone-900/40 p-4 backdrop-blur-sm sm:items-center">
      <div className={`w-full ${ancho} rounded-2xl bg-white shadow-xl`} onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <h3 className="text-base font-semibold text-stone-800">{titulo}</h3>
          <button onClick={onCerrar} className="rounded-lg p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600" aria-label="Cerrar">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6 6 18M6 6l12 12" /></svg>
          </button>
        </header>
        <div className="px-6 py-5">{children}</div>
      </div>
    </div>
  );
}

export function Tabla({ encabezados, children }: { encabezados: string[]; children: ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead>
          <tr className="border-b border-stone-200 text-xs uppercase tracking-wider text-stone-400">
            {encabezados.map((e) => (
              <th key={e} className="px-3 py-2.5 font-medium">{e}</th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-stone-100">{children}</tbody>
      </table>
    </div>
  );
}

export function Cargando() {
  return (
    <div className="flex items-center justify-center py-12 text-stone-400">
      <svg className="h-6 w-6 animate-spin" viewBox="0 0 24 24" fill="none">
        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
      </svg>
    </div>
  );
}

export function EstadoVacio({ mensaje }: { mensaje: string }) {
  return <p className="py-10 text-center text-sm text-stone-400">{mensaje}</p>;
}

export function MensajeError({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <p className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
      {error}
    </p>
  );
}
