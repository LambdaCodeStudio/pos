// Inicio: lo accionable de un vistazo — vencimientos próximos, recepciones
// pendientes de etiquetar y el estado de la caja.

import { useEffect, useState } from 'react';
import { api, tienePermiso, usuarioGuardado, type AlertaVencimiento, type RecepcionResumen, type SesionCaja } from '../lib/api';
import { cantidad, fecha } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Cargando, EstadoVacio, Insignia, Tarjeta } from './ui';

export default function Dashboard() {
  return (
    <Shell seccion="/">
      <Contenido />
    </Shell>
  );
}

function Contenido() {
  const [alertas, setAlertas] = useState<AlertaVencimiento[] | null>(null);
  const [recepciones, setRecepciones] = useState<RecepcionResumen[] | null>(null);
  const [sesiones, setSesiones] = useState<SesionCaja[] | null>(null);

  useEffect(() => {
    api<AlertaVencimiento[]>('GET', '/inventario/alertas-vencimiento?dias=15').then(setAlertas).catch(() => setAlertas([]));
    api<RecepcionResumen[]>('GET', '/compras/recepciones?estado=confirmada').then(setRecepciones).catch(() => setRecepciones([]));
    api<SesionCaja[]>('GET', '/ventas/sesiones?solo_abiertas=true').then(setSesiones).catch(() => setSesiones([]));
  }, []);

  const usuario = usuarioGuardado();
  const pendientesEtiquetar = recepciones?.filter((r) => r.items_pendientes_etiquetar > 0) ?? [];

  return (
    <>
      <Encabezado
        titulo={`Hola, ${usuario?.nombre ?? ''}`}
        subtitulo="Esto es lo que necesita atención hoy."
      />

      <div className="grid gap-5 lg:grid-cols-2">
        <Tarjeta
          titulo="Vencimientos próximos (15 días)"
          accion={<a href="/stock" className="text-xs font-medium text-acento-700 hover:underline">Ver stock →</a>}
        >
          {alertas === null ? (
            <Cargando />
          ) : alertas.length === 0 ? (
            <EstadoVacio mensaje="Nada por vencer en la ventana. 👌" />
          ) : (
            <ul className="divide-y divide-stone-100">
              {alertas.slice(0, 8).map((a) => (
                <li key={a.lote_id} className="flex items-center justify-between gap-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-stone-800">{a.producto_nombre}</p>
                    <p className="text-xs text-stone-400">
                      {cantidad(a.cantidad_actual)} u. · vence {fecha(a.vencimiento)}
                    </p>
                  </div>
                  <Insignia tono={a.dias_restantes <= 3 ? 'rojo' : a.dias_restantes <= 7 ? 'ambar' : 'neutro'}>
                    {a.dias_restantes <= 0 ? 'vencido' : `${a.dias_restantes} días`}
                  </Insignia>
                </li>
              ))}
            </ul>
          )}
        </Tarjeta>

        <div className="space-y-5">
          {tienePermiso('confirmar_recepcion') && (
            <Tarjeta
              titulo="Etiquetado pendiente"
              accion={<a href="/recepciones" className="text-xs font-medium text-acento-700 hover:underline">Recepciones →</a>}
            >
              {recepciones === null ? (
                <Cargando />
              ) : pendientesEtiquetar.length === 0 ? (
                <EstadoVacio mensaje="No hay etiquetas pendientes." />
              ) : (
                <ul className="divide-y divide-stone-100">
                  {pendientesEtiquetar.slice(0, 5).map((r) => (
                    <li key={r.id} className="flex items-center justify-between gap-3 py-2.5">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium text-stone-800">
                          {r.proveedor_nombre ?? 'Sin proveedor'}
                        </p>
                        <p className="text-xs text-stone-400">recibida el {fecha(r.confirmada_en)}</p>
                      </div>
                      <a href={`/recepcion?id=${r.id}`} className="shrink-0 text-xs font-medium text-acento-700 hover:underline">
                        {r.items_pendientes_etiquetar} ítems →
                      </a>
                    </li>
                  ))}
                </ul>
              )}
            </Tarjeta>
          )}

          <Tarjeta titulo="Cajas abiertas">
            {sesiones === null ? (
              <Cargando />
            ) : sesiones.length === 0 ? (
              <EstadoVacio mensaje="No hay ninguna caja abierta." />
            ) : (
              <ul className="divide-y divide-stone-100">
                {sesiones.map((s) => (
                  <li key={s.id} className="flex items-center justify-between gap-3 py-2.5">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-stone-800">{s.usuario_nombre}</p>
                      <p className="text-xs text-stone-400">{s.cantidad_ventas} ventas</p>
                    </div>
                    <Insignia tono="verde">abierta</Insignia>
                  </li>
                ))}
              </ul>
            )}
          </Tarjeta>
        </div>
      </div>
    </>
  );
}
