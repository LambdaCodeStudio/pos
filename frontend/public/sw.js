// Service worker del app shell. Estrategias:
//  - /_astro/* (assets hasheados, inmutables): caché primero.
//  - Navegaciones y estáticos: red primero con respaldo de caché
//    (la app abre sin conexión una vez visitada).
//  - /api/*: NUNCA se cachea — la lógica offline vive en IndexedDB
//    (catálogo local + cola de sincronización), no en el caché HTTP.

const CACHE = 'pos-shell-v1';

self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', (evento) => {
  evento.waitUntil(
    caches.keys().then((claves) =>
      Promise.all(claves.filter((c) => c !== CACHE).map((c) => caches.delete(c))),
    ).then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (evento) => {
  const url = new URL(evento.request.url);
  if (evento.request.method !== 'GET' || url.origin !== self.location.origin) return;
  if (url.pathname.startsWith('/api/')) return;

  if (url.pathname.startsWith('/_astro/')) {
    evento.respondWith(
      caches.open(CACHE).then(async (cache) => {
        const enCache = await cache.match(evento.request);
        if (enCache) return enCache;
        const respuesta = await fetch(evento.request);
        if (respuesta.ok) cache.put(evento.request, respuesta.clone());
        return respuesta;
      }),
    );
    return;
  }

  evento.respondWith(
    fetch(evento.request)
      .then((respuesta) => {
        if (respuesta.ok) {
          const copia = respuesta.clone();
          caches.open(CACHE).then((cache) => cache.put(evento.request, copia));
        }
        return respuesta;
      })
      .catch(async () => {
        const enCache = await caches.match(evento.request);
        if (enCache) return enCache;
        // Última red de seguridad para navegaciones: la caja.
        if (evento.request.mode === 'navigate') {
          const caja = await caches.match('/caja');
          if (caja) return caja;
        }
        return Response.error();
      }),
  );
});
