// Servidor de producción para el kiosco: sirve la build estática (dist/) y
// deriva /api/* al backend Axum, todo bajo un mismo origen (requisito del
// service worker y de la PWA). Sin dependencias.
//
//   node servidor.mjs            → http://0.0.0.0:8080 (toda la LAN)
//   PUERTO=80 API=http://otra-maquina:3000 node servidor.mjs
//
// Con TLS_CERT y TLS_KEY seteados (rutas a archivos PEM), sirve por HTTPS en
// vez de HTTP — necesario para WebUSB (impresión de tickets), que el
// navegador solo habilita en un contexto seguro:
//
//   TLS_CERT=server.crt TLS_KEY=server.key node servidor.mjs

import { createServer as createServerHttp, request as requestHttp } from 'node:http';
import { createServer as createServerHttps } from 'node:https';
import { createReadStream, existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const RAIZ = join(fileURLToPath(new URL('.', import.meta.url)), 'dist');
const PUERTO = Number(process.env.PUERTO ?? 8080);
const API = new URL(process.env.API ?? 'http://localhost:3000');
const TLS_CERT = process.env.TLS_CERT;
const TLS_KEY = process.env.TLS_KEY;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
};

function servirArchivo(res, ruta, inmutable = false) {
  res.writeHead(200, {
    'Content-Type': MIME[extname(ruta)] ?? 'application/octet-stream',
    'Cache-Control': inmutable ? 'public, max-age=31536000, immutable' : 'no-cache',
  });
  createReadStream(ruta).pipe(res);
}

const usaTls = Boolean(TLS_CERT && TLS_KEY);
const crearServidor = usaTls
  ? (manejador) => createServerHttps({ cert: readFileSync(TLS_CERT), key: readFileSync(TLS_KEY) }, manejador)
  : createServerHttp;

crearServidor((req, res) => {
  const url = new URL(req.url, 'http://x');

  // /api/* → backend (sin el prefijo).
  if (url.pathname.startsWith('/api/')) {
    const proxy = requestHttp(
      {
        hostname: API.hostname,
        port: API.port,
        path: url.pathname.slice(4) + url.search,
        method: req.method,
        headers: { ...req.headers, host: API.host },
      },
      (respuesta) => {
        res.writeHead(respuesta.statusCode ?? 502, respuesta.headers);
        respuesta.pipe(res);
      },
    );
    proxy.on('error', () => {
      res.writeHead(502, { 'Content-Type': 'application/json' });
      res.end('{"error":"backend no disponible"}');
    });
    req.pipe(proxy);
    return;
  }

  // Estáticos de dist/, con index.html por directorio.
  const seguro = normalize(url.pathname).replace(/^(\.\.[/\\])+/, '');
  const candidatos = [
    join(RAIZ, seguro),
    join(RAIZ, seguro, 'index.html'),
  ];
  for (const ruta of candidatos) {
    if (existsSync(ruta) && statSync(ruta).isFile()) {
      servirArchivo(res, ruta, seguro.startsWith('/_astro') || seguro.startsWith('\\_astro'));
      return;
    }
  }
  res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
  res.end('no encontrado');
}).listen(PUERTO, '0.0.0.0', () => {
  console.log(`POS sirviendo en http${usaTls ? 's' : ''}://localhost:${PUERTO} (API → ${API.origin})`);
});
