// Estado de conexión para la UI. navigator.onLine dice si hay red local;
// para la caja alcanza (el fallo real de un request igual encola).

import { useEffect, useState } from 'react';

export function useConexion(): boolean {
  const [enLinea, setEnLinea] = useState(() => navigator.onLine);
  useEffect(() => {
    const arriba = () => setEnLinea(true);
    const abajo = () => setEnLinea(false);
    window.addEventListener('online', arriba);
    window.addEventListener('offline', abajo);
    return () => {
      window.removeEventListener('online', arriba);
      window.removeEventListener('offline', abajo);
    };
  }, []);
  return enLinea;
}
