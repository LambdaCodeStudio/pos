# POS — Sistema de gestión para kioscos y almacenes

Backend en Rust (Axum + SQLx + PostgreSQL). Monolito modular: un módulo por
bounded context, un schema de PostgreSQL por contexto. La fuente de verdad
arquitectónica es [ARQUITECTURA.md](ARQUITECTURA.md) — leela antes de tocar nada.

## Estado

- **Fase 1** (lista): Identidad + Catálogo + Compras con flujo de etiquetado.
- **Fase 2** (lista): Inventario — ajustes, alertas de vencimiento, stock por producto.
- **Fase 3** (lista): Ventas/Caja — sesiones con arqueo, sync idempotente, FEFO, anulaciones.
- **Fase 4** (lista): Clientes y cuenta corriente — fiado con ledger, límite que bloquea.

Las cuatro fases del backend están completas.

- **Frontend** (`frontend/`): Astro 5 + islas React 19 + Tailwind 4, online-first
  (la capa offline/PWA para la caja se agrega después sobre esta base).
  Pantallas: login (password/PIN), inicio, caja con pagos mixtos y arqueo,
  recepciones con etiquetado, catálogo, stock con ajustes y alertas,
  clientes/fiado, equipo (usuarios y roles). La navegación se filtra por permisos.

```powershell
# Frontend en desarrollo (requiere el backend corriendo en :3000)
cd frontend; npm run dev      # http://localhost:4321 (proxy /api → :3000)

# Producción: build estática + servidor único (app + proxy /api)
cd frontend; npm run build; npm run servir   # http://<ip-del-local>:8080

# Con impresión de tickets (WebUSB): el navegador solo la habilita en un
# contexto seguro, así que en producción hace falta HTTPS (certificado
# propio alcanza en una LAN interna).
openssl req -x509 -newkey rsa:2048 -nodes -keyout server.key -out server.crt \
  -days 3650 -subj "/CN=pos.local" \
  -addext "subjectAltName=IP:<ip-del-server>,DNS:<hostname-del-server>"
TLS_CERT=server.crt TLS_KEY=server.key npm run servir   # https://<ip-del-local>:8080
# Cada dispositivo del mostrador acepta la advertencia de certificado una vez.
```

## Capa offline (PWA)

- **App shell**: service worker (`public/sw.js`) — assets hasheados caché-primero,
  páginas red-primero con respaldo; `/api` jamás se cachea. Manifest instalable
  (`display: standalone`). El SW solo se registra en la build de producción.
- **Catálogo local**: IndexedDB (`src/lib/db.ts`, `catalogoLocal.ts`) se refresca
  de `GET /catalogo/sincronizacion-caja` al entrar a la caja con conexión; sin
  red, el escaneo y la búsqueda van contra la copia local.
- **Cola de sincronización** (`src/lib/colaSync.ts`): apertura de sesión y ventas
  nacen con UUID del dispositivo y se encolan si no hay red; se empujan EN ORDEN
  al reconectar (el backend es idempotente: reintentar jamás duplica). Un rechazo
  de negocio (4xx) queda marcado "con error" para revisión manual sin frenar la cola.
- **Deliberadamente solo online**: el fiado (el límite de crédito debe bloquear
  con saldo fresco) y el cierre de caja (el arqueo lo calcula el servidor).

## Entorno de desarrollo (Windows, portable, sin admin)

- Rust vía rustup, toolchain `stable-x86_64-pc-windows-gnu`.
- MinGW-w64 portable (WinLibs) en `%USERPROFILE%\mingw64-portable` — necesario
  porque el `dlltool` de rustup no alcanza para `windows-sys` (raw-dylib).
- PostgreSQL 17 portable en `.dev/pgsql`, clúster en `.dev/pgdata` (auth trust,
  solo local), base `pos`.

```powershell
.\dev.ps1 db-start   # arranca PostgreSQL
.\dev.ps1 run        # migra (embebido al inicio) y levanta la API en :3000
.\dev.ps1 test       # tests (usan bases efímeras via #[sqlx::test])
.\dev.ps1 db-psql    # consola SQL
.\dev.ps1 db-stop    # detiene PostgreSQL
```

Variables en `.env` (no versionado): `DATABASE_URL`, `JWT_SECRET`, `PUERTO`,
opcional `ADMIN_PASSWORD_INICIAL`.

En el primer arranque sin usuarios se crea `admin` con la contraseña de
`ADMIN_PASSWORD_INICIAL` (default `admin1234`) — cambiala de inmediato.

## API (Fase 1, resumen)

- `POST /identidad/login` · `POST /identidad/login-pin` · `GET /identidad/yo`
- CRUD `/identidad/usuarios` y `/identidad/roles` (permiso `gestionar_usuarios`)
- CRUD `/catalogo/categorias` y `/catalogo/productos` (+ búsqueda `?buscar=` con pg_trgm)
- `GET /catalogo/codigos-barras/{codigo}` — hot path del escaneo
- `POST /catalogo/productos/{id}/precio` — cambio manual (ledger + proyección)
- CRUD `/compras/proveedores`
- `/compras/recepciones` — borrador → carga idempotente de ítems →
  `POST …/confirmar` (transacción crítica) → etiquetado → completada
- `POST /inventario/ajustes` — documento con motivo (`perdida|rotura|vencimiento|robo|conteo|otro`),
  ítems por `delta` o `cantidad_contada`, idempotente, valida disponibilidad
- `GET /inventario/productos/{id}/stock` — proyección + lotes con stock
- `GET /inventario/alertas-vencimiento?dias=30` — lotes por vencer, accionables
- `GET /inventario/movimientos` — consulta del ledger (permiso `ver_reportes`)
- `POST /ventas/sesiones` · `POST /ventas/sesiones/{id}/cerrar` — arqueo con
  diferencia registrada, nunca corregida
- `POST /ventas` — sincronización idempotente de ventas offline (UUID del
  dispositivo); el servidor genera los movimientos de stock con FEFO
- `POST /ventas/{id}/anular` — contra-asientos en el ledger, jamás se edita
- CRUD `/clientes` (permiso `gestionar_clientes`) — libreta de fiado
- `GET /clientes/{id}/cuenta` — ledger de cuenta corriente con saldo corrido
- `POST /clientes/{id}/pagos` · `POST /clientes/{id}/ajustes` — idempotentes
- Pago `cuenta_corriente` en `POST /ventas`: inserta el cargo en la misma
  transacción; el límite de crédito bloquea salvo `exceder_limite_credito`
- `/reportes/*` (permiso `ver_reportes`): `ventas-resumen` (serie diaria,
  medios de pago), `top-productos`, `fiado`, `inventario` (valuación a
  costo/precio, solo stock positivo), `arqueos`, `compras-resumen`.
  Consultas simples de solo lectura — no hay motor de reportes (excluido
  por diseño). Tablero en `/metricas` del frontend.
- `GET /auditoria/eventos` (permiso `ver_reportes`): eventos enriquecidos con
  quién ejecutó y el nombre de la entidad afectada; filtros por entidad,
  acción, usuario y fechas, paginado. Vista de línea de tiempo con diff
  antes/después en `/auditoria` del frontend.
# pos
# pos
# pos
