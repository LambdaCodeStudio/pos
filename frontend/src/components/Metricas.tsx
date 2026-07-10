// Tablero de métricas: ventas, medios de pago, top de productos, fiado,
// inventario, compras y arqueos. Gráficos SVG propios, sin dependencias.

import { useEffect, useMemo, useState } from 'react';
import { api } from '../lib/api';
import { cantidad as fmtCantidad, fecha, fechaHora, pesos } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Cargando, EstadoVacio, Insignia, Tabla, Tarjeta } from './ui';

type Rango = 'hoy' | '7d' | '30d' | 'mes';

interface VentasResumen {
  desde: string;
  hasta: string;
  facturado_centavos: number;
  tickets: number;
  ticket_promedio_centavos: number;
  descuentos_centavos: number;
  costo_vendido_centavos: number;
  margen_centavos: number;
  margen_pct: number;
  anuladas: number;
  anuladas_centavos: number;
  anuladas_por_motivo: { motivo: string; cantidad: number; total_centavos: number }[];
  por_dia: { fecha: string; total_centavos: number; tickets: number }[];
  por_medio: { medio: string; total_centavos: number }[];
}

interface TopProducto {
  producto_id: string;
  nombre: string;
  unidades: string;
  facturado_centavos: number;
}

interface ProductoSinMovimiento {
  producto_id: string;
  nombre: string;
  stock: string;
  valor_centavos: number;
}

interface VentaVendedor {
  usuario_id: string;
  nombre: string;
  tickets: number;
  facturado_centavos: number;
  descuentos_centavos: number;
  anuladas: number;
}

interface Mermas {
  total_centavos: number;
  por_motivo: { motivo: string; valor_centavos: number }[];
}

interface Fiado {
  en_la_calle_centavos: number;
  deudores: number;
  top_deudores: { cliente_id: string; nombre: string; saldo_centavos: number; limite_centavos: number | null }[];
}

interface Inventario {
  valor_a_costo_centavos: number;
  valor_a_precio_centavos: number;
  productos_con_stock: number;
  productos_con_stock_negativo: number;
  lotes_por_vencer_30_dias: number;
  proximos_vencimientos: {
    lote_id: string;
    producto_nombre: string;
    vencimiento: string;
    cantidad: string;
    dias_restantes: number;
  }[];
}

interface Arqueo {
  sesion_id: string;
  usuario_nombre: string;
  cerrada_en: string;
  contado_centavos: number | null;
  diferencia_centavos: number | null;
}

interface ArqueosResumen {
  total_diferencia_centavos: number;
  con_diferencia: number;
  sesiones: Arqueo[];
}

interface Compras {
  total_comprado_centavos: number;
  pendientes_confirmar: number;
  por_proveedor: { proveedor: string; recepciones: number; total_centavos: number }[];
}

const ETIQUETA_MEDIO: Record<string, string> = {
  efectivo: 'Efectivo',
  tarjeta: 'Tarjeta',
  mercado_pago: 'Mercado Pago',
  transferencia: 'Transferencia',
  cuenta_corriente: 'Fiado',
};

const ETIQUETA_MOTIVO_MERMA: Record<string, string> = {
  perdida: 'Pérdida',
  rotura: 'Rotura',
  vencimiento: 'Vencimiento',
  robo: 'Robo',
  conteo: 'Conteo',
  otro: 'Otro',
};

function fechasDeRango(rango: Rango): { desde: string; hasta: string } {
  const hoy = new Date();
  const aIso = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const hasta = aIso(hoy);
  const desdeFecha = new Date(hoy);
  if (rango === '7d') desdeFecha.setDate(hoy.getDate() - 6);
  if (rango === '30d') desdeFecha.setDate(hoy.getDate() - 29);
  if (rango === 'mes') desdeFecha.setDate(1);
  return { desde: aIso(desdeFecha), hasta };
}

export default function Metricas() {
  const [rango, setRango] = useState<Rango>('7d');
  const [ventas, setVentas] = useState<VentasResumen | null>(null);
  const [top, setTop] = useState<TopProducto[] | null>(null);
  const [sinMovimiento, setSinMovimiento] = useState<ProductoSinMovimiento[] | null>(null);
  const [vendedores, setVendedores] = useState<VentaVendedor[] | null>(null);
  const [mermas, setMermas] = useState<Mermas | null>(null);
  const [fiado, setFiado] = useState<Fiado | null>(null);
  const [inventario, setInventario] = useState<Inventario | null>(null);
  const [arqueos, setArqueos] = useState<ArqueosResumen | null>(null);
  const [compras, setCompras] = useState<Compras | null>(null);

  useEffect(() => {
    const { desde, hasta } = fechasDeRango(rango);
    const q = `?desde=${desde}&hasta=${hasta}`;
    setVentas(null);
    setTop(null);
    setSinMovimiento(null);
    setVendedores(null);
    setMermas(null);
    setCompras(null);
    api<VentasResumen>('GET', `/reportes/ventas-resumen${q}`).then(setVentas).catch(() => {});
    api<TopProducto[]>('GET', `/reportes/top-productos${q}&limite=10`).then(setTop).catch(() => {});
    api<ProductoSinMovimiento[]>('GET', `/reportes/productos-sin-movimiento${q}&limite=10`).then(setSinMovimiento).catch(() => {});
    api<VentaVendedor[]>('GET', `/reportes/ventas-por-vendedor${q}`).then(setVendedores).catch(() => {});
    api<Mermas>('GET', `/reportes/mermas${q}`).then(setMermas).catch(() => {});
    api<Compras>('GET', `/reportes/compras-resumen${q}`).then(setCompras).catch(() => {});
  }, [rango]);

  useEffect(() => {
    api<Fiado>('GET', '/reportes/fiado').then(setFiado).catch(() => {});
    api<Inventario>('GET', '/reportes/inventario').then(setInventario).catch(() => {});
    api<ArqueosResumen>('GET', '/reportes/arqueos?limite=10').then(setArqueos).catch(() => {});
  }, []);

  return (
    <Shell seccion="/metricas">
      <Encabezado
        titulo="Métricas"
        subtitulo="El pulso del negocio, calculado directo de los ledgers."
        accion={
          <div className="flex gap-1 rounded-lg bg-stone-200/70 p-1 text-sm font-medium">
            {([['hoy', 'Hoy'], ['7d', '7 días'], ['30d', '30 días'], ['mes', 'Este mes']] as const).map(([valor, etiqueta]) => (
              <button key={valor} onClick={() => setRango(valor)}
                className={`rounded-md px-3.5 py-1.5 transition ${
                  rango === valor ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
                }`}>
                {etiqueta}
              </button>
            ))}
          </div>
        }
      />

      {/* KPIs */}
      <div className="mb-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6">
        <Kpi titulo="Facturado" valor={ventas ? pesos(ventas.facturado_centavos) : '…'} destacado />
        <Kpi titulo="Margen bruto" valor={ventas ? pesos(ventas.margen_centavos) : '…'}
          nota={ventas ? `${ventas.margen_pct.toFixed(1)}% sobre facturado` : undefined} />
        <Kpi titulo="Tickets" valor={ventas ? String(ventas.tickets) : '…'}
          nota={ventas && ventas.anuladas > 0 ? `+ ${ventas.anuladas} anuladas (${pesos(ventas.anuladas_centavos)})` : undefined} />
        <Kpi titulo="Ticket promedio" valor={ventas ? pesos(ventas.ticket_promedio_centavos) : '…'} />
        <Kpi titulo="Descuentos" valor={ventas ? pesos(ventas.descuentos_centavos) : '…'} />
        <Kpi titulo="Mermas" valor={mermas ? pesos(mermas.total_centavos) : '…'} />
      </div>

      <div className="grid gap-5 lg:grid-cols-3">
        {/* Ventas por día */}
        <div className="lg:col-span-2">
          <Tarjeta titulo="Ventas por día">
            {ventas === null ? (
              <Cargando />
            ) : ventas.por_dia.length === 0 ? (
              <EstadoVacio mensaje="Sin ventas en el período." />
            ) : (
              <GraficoBarras
                datos={ventas.por_dia.map((d) => ({
                  etiqueta: d.fecha.slice(8, 10) + '/' + d.fecha.slice(5, 7),
                  valor: d.total_centavos,
                  detalle: `${fecha(d.fecha)}: ${pesos(d.total_centavos)} en ${d.tickets} tickets`,
                }))}
              />
            )}
          </Tarjeta>
        </div>

        {/* Medios de pago */}
        <Tarjeta titulo="Medios de pago">
          {ventas === null ? (
            <Cargando />
          ) : ventas.por_medio.length === 0 ? (
            <EstadoVacio mensaje="Sin cobros en el período." />
          ) : (
            <BarrasHorizontales
              datos={ventas.por_medio.map((m) => ({
                etiqueta: ETIQUETA_MEDIO[m.medio] ?? m.medio,
                valor: m.total_centavos,
              }))}
              formato={pesos}
            />
          )}
        </Tarjeta>

        {/* Top productos */}
        <div className="lg:col-span-2">
          <Tarjeta titulo="Los que más venden">
            {top === null ? (
              <Cargando />
            ) : top.length === 0 ? (
              <EstadoVacio mensaje="Sin ventas en el período." />
            ) : (
              <Tabla encabezados={['#', 'Producto', 'Unidades', 'Facturado']}>
                {top.map((p, i) => (
                  <tr key={p.producto_id} className="hover:bg-stone-50">
                    <td className="px-3 py-2.5 text-stone-400">{i + 1}</td>
                    <td className="px-3 py-2.5 font-medium text-stone-800">{p.nombre}</td>
                    <td className="px-3 py-2.5 text-stone-500">{fmtCantidad(p.unidades)}</td>
                    <td className="px-3 py-2.5 font-semibold text-stone-800">{pesos(p.facturado_centavos)}</td>
                  </tr>
                ))}
              </Tabla>
            )}
          </Tarjeta>
        </div>

        {/* Rendimiento por vendedor */}
        <div className="lg:col-span-2">
          <Tarjeta titulo="Rendimiento por vendedor">
            {vendedores === null ? (
              <Cargando />
            ) : vendedores.length === 0 ? (
              <EstadoVacio mensaje="Sin ventas en el período." />
            ) : (
              <Tabla encabezados={['Vendedor', 'Tickets', 'Facturado', 'Descuentos', 'Anuladas']}>
                {vendedores.map((v) => (
                  <tr key={v.usuario_id} className="hover:bg-stone-50">
                    <td className="px-3 py-2.5 font-medium text-stone-800">{v.nombre}</td>
                    <td className="px-3 py-2.5 text-stone-500">{v.tickets}</td>
                    <td className="px-3 py-2.5 font-semibold text-stone-800">{pesos(v.facturado_centavos)}</td>
                    <td className="px-3 py-2.5 text-stone-500">{pesos(v.descuentos_centavos)}</td>
                    <td className="px-3 py-2.5">
                      {v.anuladas > 0 ? <Insignia tono="rojo">{v.anuladas}</Insignia> : <span className="text-stone-400">0</span>}
                    </td>
                  </tr>
                ))}
              </Tabla>
            )}
          </Tarjeta>
        </div>

        {/* Productos sin rotación */}
        <Tarjeta titulo="Sin rotación en el período">
          {sinMovimiento === null ? (
            <Cargando />
          ) : sinMovimiento.length === 0 ? (
            <EstadoVacio mensaje="Todo lo que tenés en stock se vendió algo en el período." />
          ) : (
            <ul className="divide-y divide-stone-100">
              {sinMovimiento.slice(0, 8).map((p) => (
                <li key={p.producto_id} className="flex items-center justify-between gap-2 py-2 text-sm">
                  <span className="truncate text-stone-700">{p.nombre}</span>
                  <span className="shrink-0 text-right">
                    <span className="block font-semibold text-stone-800">{pesos(p.valor_centavos)}</span>
                    <span className="block text-xs text-stone-400">{fmtCantidad(p.stock)} en stock</span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Tarjeta>

        {/* Fiado */}
        <Tarjeta titulo="Fiado en la calle">
          {fiado === null ? (
            <Cargando />
          ) : (
            <>
              <p className="text-3xl font-bold text-red-600">{pesos(fiado.en_la_calle_centavos)}</p>
              <p className="mb-3 text-xs text-stone-400">{fiado.deudores} clientes con saldo</p>
              <ul className="divide-y divide-stone-100">
                {fiado.top_deudores.slice(0, 6).map((d) => (
                  <li key={d.cliente_id} className="flex justify-between py-2 text-sm">
                    <span className="text-stone-700">{d.nombre}</span>
                    <span className="font-semibold text-stone-800">{pesos(d.saldo_centavos)}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </Tarjeta>

        {/* Anulaciones */}
        <Tarjeta titulo="Anulaciones del período">
          {ventas === null ? (
            <Cargando />
          ) : ventas.anuladas === 0 ? (
            <EstadoVacio mensaje="Sin ventas anuladas en el período." />
          ) : (
            <>
              <p className="text-3xl font-bold text-red-600">{pesos(ventas.anuladas_centavos)}</p>
              <p className="mb-3 text-xs text-stone-400">{ventas.anuladas} tickets anulados</p>
              <ul className="divide-y divide-stone-100">
                {ventas.anuladas_por_motivo.map((m) => (
                  <li key={m.motivo} className="flex justify-between py-2 text-sm">
                    <span className="text-stone-700">{m.motivo} <span className="text-stone-400">({m.cantidad})</span></span>
                    <span className="font-semibold text-stone-800">{pesos(m.total_centavos)}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </Tarjeta>

        {/* Mermas */}
        <Tarjeta titulo="Mermas del período">
          {mermas === null ? (
            <Cargando />
          ) : mermas.por_motivo.length === 0 ? (
            <EstadoVacio mensaje="Sin ajustes de merma en el período." />
          ) : (
            <>
              <p className="mb-3 text-3xl font-bold text-red-600">{pesos(mermas.total_centavos)}</p>
              <BarrasHorizontales
                datos={mermas.por_motivo.map((m) => ({
                  etiqueta: ETIQUETA_MOTIVO_MERMA[m.motivo] ?? m.motivo,
                  valor: m.valor_centavos,
                }))}
                formato={pesos}
              />
            </>
          )}
        </Tarjeta>

        {/* Inventario */}
        <Tarjeta titulo="Inventario">
          {inventario === null ? (
            <Cargando />
          ) : (
            <dl className="space-y-2.5 text-sm">
              <div className="flex justify-between">
                <dt className="text-stone-500">Valor a costo</dt>
                <dd className="font-semibold text-stone-800">{pesos(inventario.valor_a_costo_centavos)}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-stone-500">Valor a precio de venta</dt>
                <dd className="font-semibold text-stone-800">{pesos(inventario.valor_a_precio_centavos)}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-stone-500">Ganancia potencial</dt>
                <dd className="font-semibold text-acento-700">
                  {pesos(inventario.valor_a_precio_centavos - inventario.valor_a_costo_centavos)}
                </dd>
              </div>
              <div className="flex justify-between border-t border-stone-100 pt-2.5">
                <dt className="text-stone-500">Productos con stock</dt>
                <dd className="text-stone-800">{inventario.productos_con_stock}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-stone-500">Con stock negativo</dt>
                <dd>
                  {inventario.productos_con_stock_negativo > 0
                    ? <Insignia tono="rojo">{inventario.productos_con_stock_negativo} a recalibrar</Insignia>
                    : <span className="text-stone-800">0</span>}
                </dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-stone-500">Lotes por vencer (30 días)</dt>
                <dd>
                  {inventario.lotes_por_vencer_30_dias > 0
                    ? <Insignia tono="ambar">{inventario.lotes_por_vencer_30_dias}</Insignia>
                    : <span className="text-stone-800">0</span>}
                </dd>
              </div>
            </dl>
          )}
          {inventario && inventario.proximos_vencimientos.length > 0 && (
            <ul className="mt-3 divide-y divide-stone-100 border-t border-stone-100">
              {inventario.proximos_vencimientos.slice(0, 6).map((l) => (
                <li key={l.lote_id} className="flex items-center justify-between gap-2 py-2 text-sm">
                  <span className="truncate text-stone-700">{l.producto_nombre} <span className="text-stone-400">({fmtCantidad(l.cantidad)})</span></span>
                  <Insignia tono={l.dias_restantes <= 7 ? 'rojo' : 'ambar'}>
                    {l.dias_restantes <= 0 ? 'vencido' : `${l.dias_restantes}d`}
                  </Insignia>
                </li>
              ))}
            </ul>
          )}
        </Tarjeta>

        {/* Compras */}
        <Tarjeta titulo="Compras del período">
          {compras === null ? (
            <Cargando />
          ) : (
            <>
              {compras.pendientes_confirmar > 0 && (
                <div className="mb-3">
                  <Insignia tono="ambar">{compras.pendientes_confirmar} recepciones sin confirmar</Insignia>
                </div>
              )}
              {compras.por_proveedor.length === 0 ? (
                <EstadoVacio mensaje="Sin recepciones confirmadas en el período." />
              ) : (
                <>
                  <p className="mb-3 text-2xl font-bold text-stone-900">{pesos(compras.total_comprado_centavos)}</p>
                  <BarrasHorizontales
                    datos={compras.por_proveedor.map((p) => ({
                      etiqueta: `${p.proveedor} (${p.recepciones})`,
                      valor: p.total_centavos,
                    }))}
                    formato={pesos}
                  />
                </>
              )}
            </>
          )}
        </Tarjeta>

        {/* Arqueos */}
        <div className="lg:col-span-2">
          <Tarjeta titulo="Últimos arqueos de caja">
            {arqueos === null ? (
              <Cargando />
            ) : arqueos.sesiones.length === 0 ? (
              <EstadoVacio mensaje="Todavía no hay cajas cerradas." />
            ) : (
              <>
                {arqueos.con_diferencia > 0 && (
                  <p className="mb-3 text-sm text-stone-500">
                    Acumulado de estos cierres:{' '}
                    <span className={`font-semibold ${arqueos.total_diferencia_centavos >= 0 ? 'text-sky-600' : 'text-red-600'}`}>
                      {arqueos.total_diferencia_centavos > 0 ? '+' : ''}{pesos(arqueos.total_diferencia_centavos)}
                    </span>{' '}
                    en {arqueos.con_diferencia} de {arqueos.sesiones.length} con diferencia.
                  </p>
                )}
                <Tabla encabezados={['Operador', 'Cierre', 'Contado', 'Diferencia']}>
                  {arqueos.sesiones.map((a) => (
                    <tr key={a.sesion_id} className="hover:bg-stone-50">
                      <td className="px-3 py-2.5 font-medium text-stone-800">{a.usuario_nombre}</td>
                      <td className="px-3 py-2.5 text-stone-400">{fechaHora(a.cerrada_en)}</td>
                      <td className="px-3 py-2.5 text-stone-500">{pesos(a.contado_centavos)}</td>
                      <td className="px-3 py-2.5">
                        {a.diferencia_centavos === 0 ? (
                          <Insignia tono="verde">exacto</Insignia>
                        ) : (
                          <span className={`font-semibold ${(a.diferencia_centavos ?? 0) > 0 ? 'text-sky-600' : 'text-red-600'}`}>
                            {(a.diferencia_centavos ?? 0) > 0 ? '+' : ''}{pesos(a.diferencia_centavos)}
                          </span>
                        )}
                      </td>
                    </tr>
                  ))}
                </Tabla>
              </>
            )}
          </Tarjeta>
        </div>
      </div>
    </Shell>
  );
}

function Kpi({ titulo, valor, nota, destacado = false }: { titulo: string; valor: string; nota?: string; destacado?: boolean }) {
  return (
    <div className={`rounded-xl border p-5 shadow-sm ${
      destacado ? 'border-acento-200 bg-acento-600 text-white' : 'border-stone-200 bg-white'
    }`}>
      <p className={`text-xs font-medium uppercase tracking-wider ${destacado ? 'text-acento-100' : 'text-stone-400'}`}>
        {titulo}
      </p>
      <p className={`mt-1 text-2xl font-bold ${destacado ? 'text-white' : 'text-stone-900'}`}>{valor}</p>
      {nota && <p className={`mt-0.5 text-xs ${destacado ? 'text-acento-100' : 'text-stone-400'}`}>{nota}</p>}
    </div>
  );
}

/** Barras verticales SVG. El detalle aparece como tooltip nativo. */
function GraficoBarras({ datos }: { datos: { etiqueta: string; valor: number; detalle: string }[] }) {
  const maximo = useMemo(() => Math.max(...datos.map((d) => d.valor), 1), [datos]);
  const ancho = 100 / datos.length;

  return (
    <div>
      <svg viewBox="0 0 100 40" className="w-full" preserveAspectRatio="none" role="img" aria-label="Ventas por día">
        {datos.map((d, i) => {
          const alto = (d.valor / maximo) * 34;
          return (
            <g key={i}>
              <rect
                x={i * ancho + ancho * 0.15}
                y={40 - alto}
                width={ancho * 0.7}
                height={alto}
                rx={0.8}
                className="fill-acento-500 transition-opacity hover:opacity-75"
              >
                <title>{d.detalle}</title>
              </rect>
            </g>
          );
        })}
      </svg>
      <div className="mt-1 flex text-[10px] text-stone-400">
        {datos.map((d, i) => (
          <span key={i} className="text-center" style={{ width: `${ancho}%` }}>
            {datos.length <= 16 || i % Math.ceil(datos.length / 16) === 0 ? d.etiqueta : ''}
          </span>
        ))}
      </div>
    </div>
  );
}

function BarrasHorizontales({
  datos,
  formato,
}: {
  datos: { etiqueta: string; valor: number }[];
  formato: (v: number) => string;
}) {
  const maximo = Math.max(...datos.map((d) => d.valor), 1);
  return (
    <ul className="space-y-3">
      {datos.map((d, i) => (
        <li key={i}>
          <div className="mb-1 flex justify-between text-sm">
            <span className="text-stone-600">{d.etiqueta}</span>
            <span className="font-semibold text-stone-800">{formato(d.valor)}</span>
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-stone-100">
            <div className="h-full rounded-full bg-acento-500" style={{ width: `${(d.valor / maximo) * 100}%` }} />
          </div>
        </li>
      ))}
    </ul>
  );
}
