// Clientes: la libreta de fiado. Lista con saldos, alta/edición con límite
// de crédito, y el detalle de cuenta con pagos y ajustes.

import { useCallback, useEffect, useState } from 'react';
import { api, tienePermiso, type Cliente, type MedioPago, type MovimientoCuenta } from '../lib/api';
import { aCentavos, fechaHora, pesos } from '../lib/formato';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

export default function Clientes() {
  const [clientes, setClientes] = useState<Cliente[] | null>(null);
  const [buscar, setBuscar] = useState('');
  const [soloConSaldo, setSoloConSaldo] = useState(false);
  const [mostrarInactivos, setMostrarInactivos] = useState(false);
  const [editando, setEditando] = useState<Cliente | 'nuevo' | null>(null);
  const [cuentaDe, setCuentaDe] = useState<Cliente | null>(null);
  const [eliminando, setEliminando] = useState<Cliente | null>(null);

  const cargar = useCallback(() => {
    const partes = ['limite=100'];
    if (buscar.trim()) partes.push(`buscar=${encodeURIComponent(buscar.trim())}`);
    if (soloConSaldo) partes.push('con_saldo=true');
    if (mostrarInactivos) partes.push('incluir_inactivos=true');
    api<Cliente[]>('GET', `/clientes?${partes.join('&')}`).then(setClientes).catch(() => setClientes([]));
  }, [buscar, soloConSaldo, mostrarInactivos]);
  useEffect(() => cargar(), [cargar]);

  const puede = tienePermiso('gestionar_clientes');

  return (
    <Shell seccion="/clientes">
      <Encabezado
        titulo="Clientes"
        subtitulo="La libreta: quién debe cuánto, con límite claro."
        accion={puede ? <Boton onClick={() => setEditando('nuevo')}>+ Nuevo cliente</Boton> : undefined}
      />

      <Tarjeta>
        <div className="mb-4 flex flex-wrap items-center gap-4">
          <input className={claseInput + ' max-w-xs'} placeholder="Buscar por nombre…"
            value={buscar} onChange={(e) => setBuscar(e.target.value)} />
          <label className="flex items-center gap-2 text-sm text-stone-600">
            <input type="checkbox" checked={soloConSaldo} onChange={(e) => setSoloConSaldo(e.target.checked)}
              className="h-4 w-4 rounded border-stone-300 text-acento-600" />
            Solo con saldo
          </label>
          <label className="flex items-center gap-2 text-sm text-stone-600">
            <input type="checkbox" checked={mostrarInactivos} onChange={(e) => setMostrarInactivos(e.target.checked)}
              className="h-4 w-4 rounded border-stone-300 text-acento-600" />
            Mostrar inactivos
          </label>
        </div>

        {clientes === null ? (
          <Cargando />
        ) : clientes.length === 0 ? (
          <EstadoVacio mensaje="Sin clientes con ese filtro." />
        ) : (
          <Tabla encabezados={['Cliente', 'Teléfono', 'Saldo', 'Límite', '', '']}>
            {clientes.map((c) => {
              const uso = c.limite_credito_centavos
                ? c.saldo_actual_centavos / c.limite_credito_centavos
                : 0;
              return (
                <tr key={c.id} className="group hover:bg-stone-50">
                  <td className="px-3 py-3 font-medium text-stone-800">{c.nombre}</td>
                  <td className="px-3 py-3 text-stone-500">{c.telefono ?? '—'}</td>
                  <td className="px-3 py-3">
                    <span className={`font-semibold ${c.saldo_actual_centavos > 0 ? 'text-red-600' : 'text-stone-800'}`}>
                      {pesos(c.saldo_actual_centavos)}
                    </span>
                  </td>
                  <td className="px-3 py-3">
                    {c.limite_credito_centavos === null ? (
                      <span className="text-stone-400">sin límite</span>
                    ) : (
                      <span className={uso >= 1 ? 'font-medium text-red-600' : 'text-stone-500'}>
                        {pesos(c.limite_credito_centavos)}
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-3">{!c.activo && <Insignia tono="rojo">inactivo</Insignia>}</td>
                  <td className="px-3 py-3 text-right">
                    <span className="flex justify-end gap-1">
                      <Boton chico variante="fantasma" onClick={() => setCuentaDe(c)}>Cuenta</Boton>
                      {puede && <Boton chico variante="fantasma" onClick={() => setEditando(c)}>Editar</Boton>}
                      {puede && c.activo && (
                        <Boton chico variante="peligro" onClick={() => setEliminando(c)}>Eliminar</Boton>
                      )}
                    </span>
                  </td>
                </tr>
              );
            })}
          </Tabla>
        )}
      </Tarjeta>

      {editando && (
        <ModalCliente cliente={editando === 'nuevo' ? null : editando}
          onCerrar={() => setEditando(null)} onGuardado={() => { setEditando(null); cargar(); }} />
      )}
      {cuentaDe && (
        <ModalCuenta cliente={cuentaDe} onCerrar={() => { setCuentaDe(null); cargar(); }} />
      )}
      {eliminando && (
        <ModalEliminarCliente cliente={eliminando} onCerrar={() => setEliminando(null)}
          onEliminado={() => { setEliminando(null); cargar(); }} />
      )}
    </Shell>
  );
}

function ModalEliminarCliente({
  cliente,
  onCerrar,
  onEliminado,
}: {
  cliente: Cliente;
  onCerrar: () => void;
  onEliminado: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [eliminando, setEliminando] = useState(false);

  async function confirmar() {
    setError(null);
    setEliminando(true);
    try {
      await api('DELETE', `/clientes/${cliente.id}`);
      onEliminado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudo eliminar el cliente.');
      setEliminando(false);
    }
  }

  return (
    <Modal abierto titulo="Eliminar cliente" onCerrar={onCerrar} ancho="max-w-sm">
      <div className="space-y-4">
        <p className="text-sm text-stone-600">
          ¿Eliminar <strong className="text-stone-800">{cliente.nombre}</strong>? Deja de listarse para
          nuevas ventas fiadas; su cuenta y su historial se conservan.
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

function ModalCliente({
  cliente,
  onCerrar,
  onGuardado,
}: {
  cliente: Cliente | null;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [nombre, setNombre] = useState(cliente?.nombre ?? '');
  const [telefono, setTelefono] = useState(cliente?.telefono ?? '');
  const [documento, setDocumento] = useState(cliente?.documento ?? '');
  const [limite, setLimite] = useState(
    cliente?.limite_credito_centavos != null ? (cliente.limite_credito_centavos / 100).toFixed(2).replace('.', ',') : '',
  );
  const [activo, setActivo] = useState(cliente?.activo ?? true);
  const [error, setError] = useState<string | null>(null);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const limiteCentavos = limite.trim() === '' ? null : aCentavos(limite);
    if (limite.trim() !== '' && limiteCentavos === null) { setError('Límite inválido'); return; }
    try {
      const cuerpo = {
        nombre,
        telefono: telefono || null,
        documento: documento || null,
        limite_credito_centavos: limiteCentavos,
        ...(cliente ? { activo } : {}),
      };
      if (cliente) await api('PATCH', `/clientes/${cliente.id}`, cuerpo);
      else await api('POST', '/clientes', cuerpo);
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={cliente ? 'Editar cliente' : 'Nuevo cliente'} onCerrar={onCerrar} ancho="max-w-sm">
      <form onSubmit={guardar} className="space-y-4">
        <Campo etiqueta="Nombre">
          <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
        </Campo>
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Teléfono">
            <input className={claseInput} value={telefono} onChange={(e) => setTelefono(e.target.value)} />
          </Campo>
          <Campo etiqueta="Documento">
            <input className={claseInput} value={documento} onChange={(e) => setDocumento(e.target.value)} />
          </Campo>
        </div>
        <Campo etiqueta="Límite de crédito ($)" ayuda="Vacío = sin límite. El límite bloquea la venta fiada.">
          <input className={claseInput} value={limite} onChange={(e) => setLimite(e.target.value)} inputMode="decimal" placeholder="sin límite" />
        </Campo>
        {cliente && (
          <label className="flex items-center gap-2 text-sm text-stone-600">
            <input type="checkbox" checked={activo} onChange={(e) => setActivo(e.target.checked)} />
            Activo
          </label>
        )}
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={!nombre.trim()}>Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}

// ---------- Cuenta corriente ----------

const ETIQUETA_TIPO = { cargo: 'Compra fiada', pago: 'Pago', ajuste: 'Ajuste' } as const;

function ModalCuenta({ cliente, onCerrar }: { cliente: Cliente; onCerrar: () => void }) {
  const [datos, setDatos] = useState<{ saldo_actual_centavos: number; movimientos: MovimientoCuenta[] } | null>(null);
  const [pagando, setPagando] = useState(false);
  const [monto, setMonto] = useState('');
  const [medio, setMedio] = useState<MedioPago>('efectivo');
  const [error, setError] = useState<string | null>(null);

  const cargar = useCallback(() => {
    api<typeof datos>('GET', `/clientes/${cliente.id}/cuenta`).then(setDatos).catch(() => {});
  }, [cliente.id]);
  useEffect(() => cargar(), [cargar]);

  async function registrarPago(e: React.FormEvent) {
    e.preventDefault();
    const centavos = aCentavos(monto);
    if (centavos === null || centavos === 0) { setError('Monto inválido'); return; }
    setError(null);
    try {
      await api('POST', `/clientes/${cliente.id}/pagos`, { monto_centavos: centavos, medio });
      setMonto('');
      setPagando(false);
      cargar();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  const puede = tienePermiso('gestionar_clientes');

  return (
    <Modal abierto titulo={`Cuenta de ${cliente.nombre}`} onCerrar={onCerrar} ancho="max-w-xl">
      {datos === null ? (
        <Cargando />
      ) : (
        <div className="space-y-4">
          <div className="flex items-end justify-between rounded-xl bg-stone-50 p-4">
            <div>
              <p className="text-xs uppercase tracking-wide text-stone-400">Debe</p>
              <p className={`text-3xl font-bold ${datos.saldo_actual_centavos > 0 ? 'text-red-600' : 'text-acento-700'}`}>
                {pesos(datos.saldo_actual_centavos)}
              </p>
            </div>
            {puede && datos.saldo_actual_centavos !== 0 && !pagando && (
              <Boton onClick={() => setPagando(true)}>Registrar pago</Boton>
            )}
          </div>

          {pagando && (
            <form onSubmit={registrarPago} className="flex flex-wrap items-end gap-2 rounded-xl border border-acento-200 bg-acento-50/50 p-4">
              <div className="min-w-[130px] flex-1">
                <Campo etiqueta="Monto ($)">
                  <input className={claseInput} value={monto} onChange={(e) => setMonto(e.target.value)} inputMode="decimal" autoFocus />
                </Campo>
              </div>
              <div className="min-w-[150px] flex-1">
                <Campo etiqueta="Medio">
                  <select className={claseInput} value={medio} onChange={(e) => setMedio(e.target.value as MedioPago)}>
                    <option value="efectivo">Efectivo</option>
                    <option value="tarjeta">Tarjeta</option>
                    <option value="mercado_pago">Mercado Pago</option>
                    <option value="transferencia">Transferencia</option>
                  </select>
                </Campo>
              </div>
              <Boton tipo="submit">Cobrar</Boton>
              <Boton variante="fantasma" onClick={() => setPagando(false)}>Cancelar</Boton>
            </form>
          )}
          <MensajeError error={error} />

          {datos.movimientos.length === 0 ? (
            <EstadoVacio mensaje="Sin movimientos todavía." />
          ) : (
            <ul className="max-h-80 divide-y divide-stone-100 overflow-y-auto">
              {datos.movimientos.map((m) => (
                <li key={m.id} className="py-2.5 text-sm">
                  <div className="flex items-center justify-between">
                    <div>
                      <p className="font-medium text-stone-800">
                        {ETIQUETA_TIPO[m.tipo]}
                        {m.motivo ? <span className="font-normal text-stone-400"> · {m.motivo}</span> : null}
                        {m.medio_pago ? <span className="font-normal text-stone-400"> · {m.medio_pago.replace('_', ' ')}</span> : null}
                      </p>
                      <p className="text-xs text-stone-400">{fechaHora(m.creado_en)}</p>
                    </div>
                    <span className={`font-semibold ${m.monto_centavos > 0 ? 'text-red-600' : 'text-acento-700'}`}>
                      {m.monto_centavos > 0 ? '+' : ''}{pesos(m.monto_centavos)}
                    </span>
                  </div>
                  {m.items.length > 0 && (
                    <ul className="mt-1.5 space-y-0.5 pl-3 text-xs text-stone-500">
                      {m.items.map((it) => (
                        <li key={it.producto_id} className="flex items-center justify-between gap-2">
                          <span>{it.producto_nombre} · {parseFloat(it.cantidad)}</span>
                          {parseFloat(it.cantidad_pendiente) > 0 ? (
                            <span className="text-red-500">pendiente: {parseFloat(it.cantidad_pendiente)}</span>
                          ) : (
                            <span className="text-acento-600">saldado</span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </Modal>
  );
}
