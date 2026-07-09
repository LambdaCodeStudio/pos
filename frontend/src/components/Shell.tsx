// Shell de la aplicación: sidebar oscura con navegación filtrada por
// permisos, header con el operador actual. Redirige a /login sin token.

import { type ReactNode, useEffect, useState } from 'react';
import { cerrarSesion, tokenGuardado, usuarioGuardado, type Usuario } from '../lib/api';

interface ItemNav {
  ruta: string;
  etiqueta: string;
  icono: ReactNode;
  /** Si se indica, el ítem solo aparece con alguno de estos permisos. */
  permisos?: string[];
}

const trazo = { fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round', strokeLinejoin: 'round' } as const;

const NAVEGACION: ItemNav[] = [
  {
    ruta: '/', etiqueta: 'Inicio',
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M3 10.5 12 3l9 7.5M5 9.5V21h14V9.5" /></svg>,
  },
  {
    ruta: '/caja', etiqueta: 'Caja', permisos: ['vender'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M3 7h18v4H3zM5 11v9h14v-9M9 15h6" /></svg>,
  },
  {
    ruta: '/recepciones', etiqueta: 'Recepciones', permisos: ['confirmar_recepcion'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M21 8 12 3 3 8v8l9 5 9-5zM3 8l9 5 9-5M12 13v8" /></svg>,
  },
  {
    ruta: '/productos', etiqueta: 'Productos',
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M4 7h16M4 7v13h16V7M4 7l2-3h12l2 3M10 11h4" /></svg>,
  },
  {
    ruta: '/stock', etiqueta: 'Stock',
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M4 20V9m5 11V4m5 16v-7m5 7V7" /></svg>,
  },
  {
    ruta: '/clientes', etiqueta: 'Clientes',
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><circle cx="9" cy="8" r="3.5" /><path d="M2.5 20c.8-3.2 3.4-5 6.5-5s5.7 1.8 6.5 5M16 4.6a3.5 3.5 0 0 1 0 6.8M18.5 15.4c1.6.7 2.6 2.2 3 4.6" /></svg>,
  },
  {
    ruta: '/proveedores', etiqueta: 'Proveedores', permisos: ['gestionar_proveedores'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M3 17V7h10v10M13 10h4l4 4v3h-2M3 17h2m4 0h6" /><circle cx="7" cy="18" r="1.8" /><circle cx="17" cy="18" r="1.8" /></svg>,
  },
  {
    ruta: '/metricas', etiqueta: 'Métricas', permisos: ['ver_reportes'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M3 3v18h18" /><path d="M7 15l4-5 3 3 5-7" /></svg>,
  },
  {
    ruta: '/usuarios', etiqueta: 'Usuarios', permisos: ['gestionar_usuarios'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><circle cx="12" cy="8" r="3.5" /><path d="M5 20c.9-3.4 3.7-5.2 7-5.2s6.1 1.8 7 5.2" /></svg>,
  },
  {
    ruta: '/auditoria', etiqueta: 'Auditoría', permisos: ['ver_reportes'],
    icono: <svg width="19" height="19" viewBox="0 0 24 24" {...trazo}><path d="M12 3l7 3v5c0 4.5-3 8.5-7 10-4-1.5-7-5.5-7-10V6z" /><path d="M9 12l2 2 4-4" /></svg>,
  },
];

/** La preferencia de menú oculto persiste: el puesto de caja lo deja cerrado. */
const CLAVE_MENU_OCULTO = 'pos_menu_oculto';

export default function Shell({
  seccion,
  children,
  amplio = false,
}: {
  seccion: string;
  children: ReactNode;
  /** Ocupa todo el ancho disponible (pantallas de operación como la caja). */
  amplio?: boolean;
}) {
  const [usuario, setUsuario] = useState<Usuario | null>(null);
  const [menuOculto, setMenuOculto] = useState(false);

  useEffect(() => {
    if (!tokenGuardado()) {
      window.location.href = '/login';
      return;
    }
    setUsuario(usuarioGuardado());
    setMenuOculto(localStorage.getItem(CLAVE_MENU_OCULTO) === '1');
  }, []);

  function alternarMenu() {
    setMenuOculto((oculto) => {
      localStorage.setItem(CLAVE_MENU_OCULTO, oculto ? '0' : '1');
      return !oculto;
    });
  }

  if (!usuario) return null;

  const visibles = NAVEGACION.filter(
    (item) => !item.permisos || item.permisos.some((p) => usuario.permisos.includes(p)),
  );

  return (
    <div className="flex min-h-screen">
      {menuOculto && (
        <button
          onClick={alternarMenu}
          aria-label="Mostrar menú"
          title="Mostrar menú"
          className="fixed left-3 top-3 z-50 rounded-lg border border-stone-300 bg-white p-2 text-stone-600 shadow-sm transition hover:bg-stone-50"
        >
          <svg width="18" height="18" viewBox="0 0 24 24" {...trazo}><path d="M4 6h16M4 12h16M4 18h16" /></svg>
        </button>
      )}

      <aside className={`fixed inset-y-0 left-0 z-40 flex w-56 flex-col bg-stone-900 text-stone-300 transition-transform ${
        menuOculto ? '-translate-x-full' : 'translate-x-0'
      }`}>
        <div className="flex items-center gap-2.5 px-5 py-5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-acento-600 font-bold text-white">P</div>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-semibold text-white">Punto de venta</p>
            <p className="text-[11px] text-stone-400">gestión del almacén</p>
          </div>
          <button
            onClick={alternarMenu}
            aria-label="Ocultar menú"
            title="Ocultar menú"
            className="rounded-lg p-1.5 text-stone-500 transition hover:bg-white/10 hover:text-white"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" {...trazo}><path d="M15 6l-6 6 6 6" /></svg>
          </button>
        </div>
        <nav className="mt-2 flex-1 space-y-0.5 px-3">
          {visibles.map((item) => {
            const activo = seccion === item.ruta;
            return (
              <a
                key={item.ruta}
                href={item.ruta}
                className={`flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition ${
                  activo
                    ? 'bg-acento-600/15 font-medium text-acento-300'
                    : 'hover:bg-white/5 hover:text-white'
                }`}
              >
                {item.icono}
                {item.etiqueta}
              </a>
            );
          })}
        </nav>
        <div className="border-t border-white/10 p-4">
          <p className="truncate text-sm font-medium text-white">{usuario.nombre}</p>
          <button onClick={cerrarSesion} className="mt-1 text-xs text-stone-400 transition hover:text-white">
            Cerrar sesión →
          </button>
        </div>
      </aside>

      <main className={`flex-1 py-7 pr-8 transition-[margin] ${menuOculto ? 'ml-0 pl-16' : 'ml-56 pl-8'}`}>
        <div className={`mx-auto ${amplio ? 'max-w-none' : 'max-w-6xl'}`}>{children}</div>
      </main>
    </div>
  );
}

export function Encabezado({ titulo, subtitulo, accion }: { titulo: string; subtitulo?: string; accion?: ReactNode }) {
  return (
    <header className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight text-stone-900">{titulo}</h1>
        {subtitulo && <p className="mt-1 text-sm text-stone-500">{subtitulo}</p>}
      </div>
      {accion}
    </header>
  );
}
