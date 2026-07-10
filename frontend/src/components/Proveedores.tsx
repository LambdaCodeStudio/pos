import { useCallback, useEffect, useState } from 'react';
import { api, tienePermiso, type Proveedor } from '../lib/api';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

export default function Proveedores() {
  const [proveedores, setProveedores] = useState<Proveedor[] | null>(null);
  const [editando, setEditando] = useState<Proveedor | 'nuevo' | null>(null);
  const [eliminando, setEliminando] = useState<Proveedor | null>(null);
  const [mostrarInactivos, setMostrarInactivos] = useState(false);

  const cargar = useCallback((inactivos: boolean) => {
    const q = inactivos ? '?incluir_inactivos=true' : '';
    api<Proveedor[]>('GET', `/compras/proveedores${q}`).then(setProveedores).catch(() => setProveedores([]));
  }, []);
  useEffect(() => cargar(mostrarInactivos), [cargar, mostrarInactivos]);

  const puedeGestionar = tienePermiso('gestionar_proveedores');

  return (
    <Shell seccion="/proveedores">
      <Encabezado
        titulo="Proveedores"
        subtitulo="De quién comprás y cómo pasan sus precios."
        accion={puedeGestionar && <Boton onClick={() => setEditando('nuevo')}>+ Nuevo proveedor</Boton>}
      />
      <Tarjeta>
        <label className="mb-4 flex items-center gap-2 text-sm text-stone-500">
          <input
            type="checkbox"
            checked={mostrarInactivos}
            onChange={(e) => setMostrarInactivos(e.target.checked)}
          />
          Mostrar inactivos
        </label>
        {proveedores === null ? (
          <Cargando />
        ) : proveedores.length === 0 ? (
          <EstadoVacio mensaje="Sin proveedores todavía." />
        ) : (
          <Tabla encabezados={['Proveedor', 'CUIT', 'Teléfono', 'Precios', 'Condiciones', '', '']}>
            {proveedores.map((p) => (
              <tr key={p.id} className="group hover:bg-stone-50">
                <td className="px-3 py-3 font-medium text-stone-800">{p.nombre}</td>
                <td className="px-3 py-3 text-stone-500">{p.cuit ?? '—'}</td>
                <td className="px-3 py-3 text-stone-500">{p.telefono ?? '—'}</td>
                <td className="px-3 py-3">
                  <Insignia tono={p.precios_con_iva ? 'verde' : 'azul'}>
                    {p.precios_con_iva ? 'con IVA' : 'sin IVA'}
                  </Insignia>
                </td>
                <td className="px-3 py-3 text-stone-500">{p.condiciones_pago ?? '—'}</td>
                <td className="px-3 py-3">{!p.activo && <Insignia tono="rojo">inactivo</Insignia>}</td>
                <td className="px-3 py-3 text-right">
                  <span className="flex justify-end gap-1">
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
      </Tarjeta>

      {editando && (
        <ModalProveedor
          proveedor={editando === 'nuevo' ? null : editando}
          onCerrar={() => setEditando(null)}
          onGuardado={() => { setEditando(null); cargar(mostrarInactivos); }}
        />
      )}
      {eliminando && (
        <ModalEliminarProveedor
          proveedor={eliminando}
          onCerrar={() => setEliminando(null)}
          onEliminado={() => { setEliminando(null); cargar(mostrarInactivos); }}
        />
      )}
    </Shell>
  );
}

function ModalEliminarProveedor({
  proveedor,
  onCerrar,
  onEliminado,
}: {
  proveedor: Proveedor;
  onCerrar: () => void;
  onEliminado: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [eliminando, setEliminando] = useState(false);

  async function confirmar() {
    setError(null);
    setEliminando(true);
    try {
      await api('DELETE', `/compras/proveedores/${proveedor.id}`);
      onEliminado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'No se pudo eliminar el proveedor.');
      setEliminando(false);
    }
  }

  return (
    <Modal abierto titulo="Eliminar proveedor" onCerrar={onCerrar} ancho="max-w-sm">
      <div className="space-y-4">
        <p className="text-sm text-stone-600">
          ¿Eliminar <strong className="text-stone-800">{proveedor.nombre}</strong>? Deja de listarse para
          nuevas recepciones; su historial de compras se conserva.
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

function ModalProveedor({
  proveedor,
  onCerrar,
  onGuardado,
}: {
  proveedor: Proveedor | null;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [nombre, setNombre] = useState(proveedor?.nombre ?? '');
  const [cuit, setCuit] = useState(proveedor?.cuit ?? '');
  const [telefono, setTelefono] = useState(proveedor?.telefono ?? '');
  const [preciosConIva, setPreciosConIva] = useState(proveedor?.precios_con_iva ?? true);
  const [condiciones, setCondiciones] = useState(proveedor?.condiciones_pago ?? '');
  const [activo, setActivo] = useState(proveedor?.activo ?? true);
  const [error, setError] = useState<string | null>(null);

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const cuerpo = {
      nombre,
      cuit: cuit || null,
      telefono: telefono || null,
      precios_con_iva: preciosConIva,
      condiciones_pago: condiciones || null,
      ...(proveedor ? { activo } : {}),
    };
    try {
      if (proveedor) await api('PATCH', `/compras/proveedores/${proveedor.id}`, cuerpo);
      else await api('POST', '/compras/proveedores', cuerpo);
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={proveedor ? 'Editar proveedor' : 'Nuevo proveedor'} onCerrar={onCerrar}>
      <form onSubmit={guardar} className="space-y-4">
        <Campo etiqueta="Nombre">
          <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
        </Campo>
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="CUIT">
            <input className={claseInput} value={cuit} onChange={(e) => setCuit(e.target.value)} />
          </Campo>
          <Campo etiqueta="Teléfono">
            <input className={claseInput} value={telefono} onChange={(e) => setTelefono(e.target.value)} />
          </Campo>
        </div>
        <Campo etiqueta="Sus precios de lista…" ayuda="Es el default al cargar sus recepciones; se puede pisar por ítem">
          <select className={claseInput} value={preciosConIva ? 'si' : 'no'}
            onChange={(e) => setPreciosConIva(e.target.value === 'si')}>
            <option value="si">Ya incluyen IVA</option>
            <option value="no">No incluyen IVA</option>
          </select>
        </Campo>
        <Campo etiqueta="Condiciones de pago">
          <input className={claseInput} value={condiciones} onChange={(e) => setCondiciones(e.target.value)}
            placeholder="cuenta corriente 15 días, contado, etc." />
        </Campo>
        {proveedor && (
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
