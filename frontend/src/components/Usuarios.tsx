// Administración de usuarios y roles. Los permisos son un catálogo fijo del
// software; los roles son bundles editables; lo individual es solo aditivo.

import { useCallback, useEffect, useState } from 'react';
import { api, type RolConPermisos, type UsuarioResumen } from '../lib/api';
import Shell, { Encabezado } from './Shell';
import { Boton, Campo, Cargando, claseInput, EstadoVacio, Insignia, MensajeError, Modal, Tabla, Tarjeta } from './ui';

const ETIQUETAS_PERMISOS: Record<string, string> = {
  vender: 'Vender',
  anular_venta: 'Anular ventas',
  aplicar_descuento: 'Aplicar descuentos',
  exceder_limite_credito: 'Exceder límite de crédito',
  confirmar_recepcion: 'Confirmar recepciones',
  ajustar_stock: 'Ajustar stock',
  modificar_precios: 'Modificar precios',
  gestionar_usuarios: 'Gestionar usuarios',
  gestionar_clientes: 'Gestionar clientes',
  ver_reportes: 'Ver reportes',
  cerrar_caja: 'Cerrar caja',
  abrir_caja: 'Abrir caja',
  gestionar_catalogo: 'Gestionar catálogo',
  gestionar_proveedores: 'Gestionar proveedores',
};

export default function Usuarios() {
  const [pestana, setPestana] = useState<'usuarios' | 'roles'>('usuarios');
  return (
    <Shell seccion="/usuarios">
      <Encabezado
        titulo="Equipo"
        subtitulo="Quién entra al sistema y qué puede hacer."
        accion={
          <div className="grid grid-cols-2 rounded-lg bg-stone-200/70 p-1 text-sm font-medium">
            {(['usuarios', 'roles'] as const).map((p) => (
              <button key={p} onClick={() => setPestana(p)}
                className={`rounded-md px-4 py-1.5 capitalize transition ${
                  pestana === p ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
                }`}>
                {p}
              </button>
            ))}
          </div>
        }
      />
      {pestana === 'usuarios' ? <TablaUsuarios /> : <TablaRoles />}
    </Shell>
  );
}

// ---------- Usuarios ----------

function TablaUsuarios() {
  const [usuarios, setUsuarios] = useState<UsuarioResumen[] | null>(null);
  const [roles, setRoles] = useState<RolConPermisos[]>([]);
  const [editando, setEditando] = useState<UsuarioResumen | 'nuevo' | null>(null);

  const cargar = useCallback(() => {
    api<UsuarioResumen[]>('GET', '/identidad/usuarios?incluir_inactivos=true').then(setUsuarios).catch(() => setUsuarios([]));
    api<RolConPermisos[]>('GET', '/identidad/roles').then(setRoles).catch(() => {});
  }, []);
  useEffect(() => cargar(), [cargar]);

  return (
    <Tarjeta>
      <div className="mb-4 flex justify-end">
        <Boton onClick={() => setEditando('nuevo')}>+ Nuevo usuario</Boton>
      </div>
      {usuarios === null ? (
        <Cargando />
      ) : (
        <Tabla encabezados={['Usuario', 'Rol', 'PIN', 'Permisos extra', '', '']}>
          {usuarios.map((u) => (
            <tr key={u.id} className="group hover:bg-stone-50">
              <td className="px-3 py-3 font-medium text-stone-800">{u.nombre}</td>
              <td className="px-3 py-3"><Insignia tono="azul">{u.rol_nombre}</Insignia></td>
              <td className="px-3 py-3 text-stone-500">{u.tiene_pin ? '●●●●' : '—'}</td>
              <td className="px-3 py-3 text-xs text-stone-500">
                {u.permisos_extra.length > 0
                  ? u.permisos_extra.map((p) => ETIQUETAS_PERMISOS[p] ?? p).join(', ')
                  : '—'}
              </td>
              <td className="px-3 py-3">{!u.activo && <Insignia tono="rojo">inactivo</Insignia>}</td>
              <td className="px-3 py-3 text-right">
                <span className="invisible group-hover:visible">
                  <Boton chico variante="fantasma" onClick={() => setEditando(u)}>Editar</Boton>
                </span>
              </td>
            </tr>
          ))}
        </Tabla>
      )}
      {editando && (
        <ModalUsuario usuario={editando === 'nuevo' ? null : editando} roles={roles}
          onCerrar={() => setEditando(null)} onGuardado={() => { setEditando(null); cargar(); }} />
      )}
    </Tarjeta>
  );
}

function ModalUsuario({
  usuario, roles, onCerrar, onGuardado,
}: {
  usuario: UsuarioResumen | null;
  roles: RolConPermisos[];
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [nombre, setNombre] = useState(usuario?.nombre ?? '');
  const [rolId, setRolId] = useState(usuario?.rol_id ?? roles[0]?.id ?? '');
  const [password, setPassword] = useState('');
  const [pin, setPin] = useState('');
  const [extra, setExtra] = useState<string[]>(usuario?.permisos_extra ?? []);
  const [activo, setActivo] = useState(usuario?.activo ?? true);
  const [error, setError] = useState<string | null>(null);

  function alternarPermiso(p: string) {
    setExtra((prev) => (prev.includes(p) ? prev.filter((x) => x !== p) : [...prev, p]));
  }

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      if (usuario) {
        await api('PATCH', `/identidad/usuarios/${usuario.id}`, {
          nombre,
          rol_id: rolId,
          password: password || null,
          pin: pin || null,
          permisos_extra: extra,
          activo,
        });
      } else {
        await api('POST', '/identidad/usuarios', {
          nombre,
          rol_id: rolId,
          password,
          pin: pin || null,
          permisos_extra: extra,
        });
      }
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  const rolElegido = roles.find((r) => r.id === rolId);

  return (
    <Modal abierto titulo={usuario ? `Editar a ${usuario.nombre}` : 'Nuevo usuario'} onCerrar={onCerrar}>
      <form onSubmit={guardar} className="space-y-4">
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Nombre de usuario">
            <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
          </Campo>
          <Campo etiqueta="Rol">
            <select className={claseInput} value={rolId} onChange={(e) => setRolId(e.target.value)}>
              {roles.filter((r) => r.activo).map((r) => (
                <option key={r.id} value={r.id}>{r.nombre}</option>
              ))}
            </select>
          </Campo>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta={usuario ? 'Nueva contraseña (opcional)' : 'Contraseña'} ayuda="Mínimo 8 caracteres">
            <input type="password" className={claseInput} value={password} onChange={(e) => setPassword(e.target.value)} />
          </Campo>
          <Campo etiqueta={usuario ? 'Nuevo PIN (opcional)' : 'PIN de caja (opcional)'} ayuda="4 a 6 dígitos">
            <input className={claseInput} value={pin} inputMode="numeric" maxLength={6}
              onChange={(e) => setPin(e.target.value.replace(/\D/g, ''))} />
          </Campo>
        </div>

        <div>
          <p className="mb-2 text-sm font-medium text-stone-600">Permisos extra (además del rol)</p>
          <div className="grid grid-cols-2 gap-1.5">
            {Object.entries(ETIQUETAS_PERMISOS).map(([permiso, etiqueta]) => {
              const delRol = rolElegido?.permisos.includes(permiso) ?? false;
              return (
                <label key={permiso}
                  className={`flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm ${delRol ? 'text-stone-300' : 'text-stone-700'}`}>
                  <input type="checkbox" disabled={delRol} checked={delRol || extra.includes(permiso)}
                    onChange={() => alternarPermiso(permiso)}
                    className="h-4 w-4 rounded border-stone-300 text-acento-600" />
                  {etiqueta}
                  {delRol && <span className="text-[10px] uppercase">(rol)</span>}
                </label>
              );
            })}
          </div>
        </div>

        {usuario && (
          <label className="flex items-center gap-2 text-sm text-stone-700">
            <input type="checkbox" checked={activo} onChange={(e) => setActivo(e.target.checked)}
              className="h-4 w-4 rounded border-stone-300 text-acento-600" />
            Usuario activo
          </label>
        )}

        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={!nombre.trim() || (!usuario && password.length < 8)}>Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}

// ---------- Roles ----------

function TablaRoles() {
  const [roles, setRoles] = useState<RolConPermisos[] | null>(null);
  const [editando, setEditando] = useState<RolConPermisos | 'nuevo' | null>(null);

  const cargar = useCallback(() => {
    api<RolConPermisos[]>('GET', '/identidad/roles').then(setRoles).catch(() => setRoles([]));
  }, []);
  useEffect(() => cargar(), [cargar]);

  return (
    <Tarjeta>
      <div className="mb-4 flex justify-end">
        <Boton onClick={() => setEditando('nuevo')}>+ Nuevo rol</Boton>
      </div>
      {roles === null ? (
        <Cargando />
      ) : roles.length === 0 ? (
        <EstadoVacio mensaje="Sin roles." />
      ) : (
        <div className="grid gap-4 sm:grid-cols-2">
          {roles.map((r) => (
            <div key={r.id} className="rounded-xl border border-stone-200 p-4">
              <div className="mb-2 flex items-center justify-between">
                <p className="font-semibold text-stone-800">{r.nombre}</p>
                <Boton chico variante="fantasma" onClick={() => setEditando(r)}>Editar</Boton>
              </div>
              {r.descripcion && <p className="mb-2 text-xs text-stone-400">{r.descripcion}</p>}
              <div className="flex flex-wrap gap-1">
                {r.permisos.map((p) => (
                  <Insignia key={p}>{ETIQUETAS_PERMISOS[p] ?? p}</Insignia>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
      {editando && (
        <ModalRol rol={editando === 'nuevo' ? null : editando}
          onCerrar={() => setEditando(null)} onGuardado={() => { setEditando(null); cargar(); }} />
      )}
    </Tarjeta>
  );
}

function ModalRol({
  rol, onCerrar, onGuardado,
}: {
  rol: RolConPermisos | null;
  onCerrar: () => void;
  onGuardado: () => void;
}) {
  const [nombre, setNombre] = useState(rol?.nombre ?? '');
  const [descripcion, setDescripcion] = useState(rol?.descripcion ?? '');
  const [permisos, setPermisos] = useState<string[]>(rol?.permisos ?? []);
  const [error, setError] = useState<string | null>(null);

  function alternar(p: string) {
    setPermisos((prev) => (prev.includes(p) ? prev.filter((x) => x !== p) : [...prev, p]));
  }

  async function guardar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      const cuerpo = { nombre, descripcion: descripcion || null, permisos };
      if (rol) await api('PATCH', `/identidad/roles/${rol.id}`, cuerpo);
      else await api('POST', '/identidad/roles', cuerpo);
      onGuardado();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'error');
    }
  }

  return (
    <Modal abierto titulo={rol ? `Editar rol ${rol.nombre}` : 'Nuevo rol'} onCerrar={onCerrar}>
      <form onSubmit={guardar} className="space-y-4">
        <div className="grid grid-cols-2 gap-4">
          <Campo etiqueta="Nombre">
            <input className={claseInput} value={nombre} onChange={(e) => setNombre(e.target.value)} autoFocus />
          </Campo>
          <Campo etiqueta="Descripción">
            <input className={claseInput} value={descripcion} onChange={(e) => setDescripcion(e.target.value)} />
          </Campo>
        </div>
        <div>
          <p className="mb-2 text-sm font-medium text-stone-600">Permisos del rol</p>
          <div className="grid grid-cols-2 gap-1.5">
            {Object.entries(ETIQUETAS_PERMISOS).map(([permiso, etiqueta]) => (
              <label key={permiso} className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-stone-700">
                <input type="checkbox" checked={permisos.includes(permiso)} onChange={() => alternar(permiso)}
                  className="h-4 w-4 rounded border-stone-300 text-acento-600" />
                {etiqueta}
              </label>
            ))}
          </div>
        </div>
        <MensajeError error={error} />
        <div className="flex justify-end gap-2">
          <Boton variante="secundario" onClick={onCerrar}>Cancelar</Boton>
          <Boton tipo="submit" deshabilitado={!nombre.trim()}>Guardar</Boton>
        </div>
      </form>
    </Modal>
  );
}
