// Login doble: contraseña (acceso administrativo) o PIN (cambio rápido de
// operador en la caja compartida).

import { useState } from 'react';
import { api, guardarSesion, type Usuario } from '../lib/api';
import { Boton, Campo, claseInput, MensajeError } from './ui';

interface RespuestaLogin {
  token: string;
  usuario: Usuario;
}

export default function Login() {
  const [modo, setModo] = useState<'pin' | 'password'>('pin');
  const [nombre, setNombre] = useState('');
  const [secreto, setSecreto] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(false);

  async function entrar(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setCargando(true);
    try {
      const ruta = modo === 'pin' ? '/identidad/login-pin' : '/identidad/login';
      const cuerpo = modo === 'pin' ? { nombre, pin: secreto } : { nombre, password: secreto };
      const r = await api<RespuestaLogin>('POST', ruta, cuerpo);
      guardarSesion(r.token, r.usuario);
      window.location.href = '/';
    } catch (err) {
      setError(err instanceof Error && err.message !== 'no autenticado'
        ? err.message
        : 'Usuario o credencial incorrectos');
      setCargando(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-stone-900 p-4">
      <div className="w-full max-w-sm">
        <div className="mb-8 text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-acento-600 text-2xl font-bold text-white shadow-lg shadow-acento-900/40">
            P
          </div>
          <h1 className="text-xl font-semibold text-white">Punto de venta</h1>
          <p className="mt-1 text-sm text-stone-400">Gestión del almacén</p>
        </div>

        <form onSubmit={entrar} className="rounded-2xl bg-white p-6 shadow-xl">
          <div className="mb-5 grid grid-cols-2 rounded-lg bg-stone-100 p-1 text-sm font-medium">
            {(['pin', 'password'] as const).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => { setModo(m); setSecreto(''); setError(null); }}
                className={`rounded-md py-1.5 transition ${
                  modo === m ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
                }`}
              >
                {m === 'pin' ? 'PIN' : 'Contraseña'}
              </button>
            ))}
          </div>

          <div className="space-y-4">
            <Campo etiqueta="Usuario">
              <input
                className={claseInput}
                value={nombre}
                onChange={(e) => setNombre(e.target.value)}
                autoFocus
                autoComplete="username"
              />
            </Campo>
            <Campo etiqueta={modo === 'pin' ? 'PIN (4 a 6 dígitos)' : 'Contraseña'}>
              <input
                className={`${claseInput} ${modo === 'pin' ? 'text-center text-xl tracking-[0.5em]' : ''}`}
                type="password"
                inputMode={modo === 'pin' ? 'numeric' : undefined}
                maxLength={modo === 'pin' ? 6 : undefined}
                value={secreto}
                onChange={(e) => setSecreto(e.target.value)}
                autoComplete="current-password"
              />
            </Campo>
            <MensajeError error={error} />
            <div className="pt-1">
              <Boton tipo="submit" deshabilitado={cargando || !nombre || !secreto}>
                {cargando ? 'Entrando…' : 'Entrar'}
              </Boton>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
