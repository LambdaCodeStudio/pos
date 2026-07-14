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
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden bg-stone-900 p-4">
      {/* Fondo: resplandor radial + textura sutil */}
      <div className="pointer-events-none absolute inset-0">
        <div className="absolute left-1/2 top-0 h-[36rem] w-[36rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-acento-600/25 blur-3xl" />
        <div className="absolute bottom-0 right-0 h-72 w-72 translate-x-1/3 translate-y-1/3 rounded-full bg-acento-800/20 blur-3xl" />
        <div
          className="absolute inset-0 opacity-[0.04]"
          style={{
            backgroundImage:
              'linear-gradient(to right, #fff 1px, transparent 1px), linear-gradient(to bottom, #fff 1px, transparent 1px)',
            backgroundSize: '32px 32px',
          }}
        />
      </div>

      <div className="relative w-full max-w-sm animate-[fadeIn_0.4s_ease-out]">
        <div className="mb-8 text-center">
          <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-acento-400 to-acento-700 text-2xl font-bold text-white shadow-lg shadow-acento-900/50 ring-1 ring-white/10">
            P
          </div>
          <h1 className="text-xl font-semibold tracking-tight text-white">Punto de venta</h1>
          <p className="mt-1 text-sm text-stone-400">Gestión del almacén</p>
        </div>

        <form
          onSubmit={entrar}
          className="rounded-2xl border border-white/5 bg-white p-6 shadow-2xl shadow-black/40"
        >
          <div className="mb-5 grid grid-cols-2 rounded-lg bg-stone-100 p-1 text-sm font-medium">
            {(['pin', 'password'] as const).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => { setModo(m); setSecreto(''); setError(null); }}
                className={`flex items-center justify-center gap-1.5 rounded-md py-1.5 transition-all duration-150 ${
                  modo === m ? 'bg-white text-stone-800 shadow-sm' : 'text-stone-500 hover:text-stone-700'
                }`}
              >
                {m === 'pin' ? (
                  <svg viewBox="0 0 24 24" fill="none" className="h-4 w-4">
                    <rect x="3" y="9" width="18" height="10" rx="2" stroke="currentColor" strokeWidth="1.6" />
                    <circle cx="8" cy="14" r="1" fill="currentColor" />
                    <circle cx="12" cy="14" r="1" fill="currentColor" />
                    <circle cx="16" cy="14" r="1" fill="currentColor" />
                  </svg>
                ) : (
                  <svg viewBox="0 0 24 24" fill="none" className="h-4 w-4">
                    <rect x="4" y="10" width="16" height="10" rx="2" stroke="currentColor" strokeWidth="1.6" />
                    <path d="M8 10V7a4 4 0 0 1 8 0v3" stroke="currentColor" strokeWidth="1.6" />
                  </svg>
                )}
                {m === 'pin' ? 'PIN' : 'Contraseña'}
              </button>
            ))}
          </div>

          <div className="space-y-4">
            <Campo etiqueta="Usuario">
              <div className="relative">
                <svg viewBox="0 0 24 24" fill="none" className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-stone-400">
                  <circle cx="12" cy="8" r="3.2" stroke="currentColor" strokeWidth="1.6" />
                  <path d="M5 20c1.2-3.6 4-5.4 7-5.4s5.8 1.8 7 5.4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
                </svg>
                <input
                  className={`${claseInput} pl-9`}
                  value={nombre}
                  onChange={(e) => setNombre(e.target.value)}
                  autoFocus
                  autoComplete="username"
                />
              </div>
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
                {cargando && (
                  <svg viewBox="0 0 24 24" fill="none" className="h-4 w-4 animate-spin">
                    <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="3" opacity="0.25" />
                    <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
                  </svg>
                )}
                {cargando ? 'Entrando…' : 'Entrar'}
              </Boton>
            </div>
          </div>
        </form>

        <p className="mt-6 text-center text-xs text-stone-500">Acceso restringido al personal del local</p>
      </div>
    </div>
  );
}
