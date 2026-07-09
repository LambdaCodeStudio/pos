import { useCallback, useEffect, useState } from 'react';
import { api, type Proveedor } from '../lib/api';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

export default function Proveedores() {
  const [proveedores, setProveedores] = useState<Proveedor[] | null>(null);
  const [editando, setEditando] = useState<Proveedor | 'nuevo' | null>(null);

  const cargar = useCallback(() => {
    api<Proveedor[]>('GET', '/compras/proveedores').then(setProveedores).catch(() => setProveedores([]));
  }, []);
  useEffect(() => cargar(), [cargar]);

  return (
    <Shell seccion="/proveedores">
      <Encabezado
        titulo="Proveedores"
        subtitulo="De quién comprás y cómo pasan sus precios."
        accion={<Boton onClick={() => setEditando('nuevo')}>+ Nuevo proveedor</Boton>}
      />
      <Tarjeta>
        {proveedores === null ? (
          <Cargando />
        ) : proveedores.length === 0 ? (
          <EstadoVacio mensaje="Sin proveedores todavía." />
        ) : (
          <Tabla encabezados={['Proveedor', 'CUIT', 'Teléfono', 'Precios', 'Condiciones', '']}>
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
                <td className="px-3 py-3 text-right">
                  <span className="invisible group-hover:visible">
                    <Boton chico variante="fantasma" onClick={() => setEditando(p)}>Editar</Boton>
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
          onGuardado={() => { setEditando(null); cargar(); }}
        />
      )}
    </Shell>
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
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={!nombre.trim()}>Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}
