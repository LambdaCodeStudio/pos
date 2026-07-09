# PROMPT PARA CLAUDE CODE — Sistema de Gestión para Kioscos/Almacenes ("ERP incremental")

Sos el desarrollador principal de un sistema de gestión para kioscos, almacenes y
minimercados, con arquitectura preparada para crecer a supermercados pequeños.
Este documento es el resultado de un proceso de diseño arquitectónico completo:
**las decisiones aquí registradas ya fueron debatidas y aprobadas. No las
reabras salvo que encuentres una contradicción interna — en ese caso, frenás y
preguntás antes de codear.**

---

## 1. STACK Y ARQUITECTURA FÍSICA (decidido, no negociable)

- **Backend: Rust**, framework **Axum**, acceso a datos con **SQLx** (queries
  verificadas), **PostgreSQL** como única base.
- **Monolito modular**: un solo binario, un módulo Rust por bounded context,
  un **schema de PostgreSQL por contexto** (`catalogo`, `compras`, `inventario`,
  `ventas`, `clientes`, `identidad`, `auditoria`). NO microservicios.
- Migraciones con `sqlx migrate`. API REST/JSON.
- Frontend fuera del alcance de este prompt (habrá una PWA offline-first como
  cliente; el backend debe asumirlo — ver convención de idempotencia).

## 2. CONVENCIONES GLOBALES (aplican a TODO el código)

1. **IDs: UUIDv7 en todas las tablas.** Razón: offline-first — las ventas se
   crean en el dispositivo sin servidor. Nunca secuencias para entidades de
   negocio.
2. **Dinero: centavos enteros (`BIGINT`). Jamás float, jamás pesos con
   decimales.** Porcentajes (IVA, markup): `NUMERIC(5,2)`. Cálculos monetarios
   en Rust con `rust_decimal::Decimal`, redondeo solo al final.
3. **Cantidades: `NUMERIC(12,3)` en todo el sistema** (soporta productos por
   peso desde el día cero aunque hoy solo se vendan unidades).
4. **Patrón central: ledger inmutable + proyección.** Los hechos (movimientos
   de stock, historial de precios, movimientos de cuenta corriente) son tablas
   solo-INSERT. Los estados actuales (`stock_actual`, `precio_actual_centavos`,
   `saldo_actual`) son proyecciones denormalizadas que se actualizan **en la
   misma transacción** que inserta el hecho. Regla de oro: toda proyección debe
   poder reconstruirse desde su ledger. Correcciones = contra-asientos, nunca
   UPDATE/DELETE sobre ledgers.
5. **Idempotencia en toda la API de escritura**: reintentar un request ya
   procesado no duplica nada (el UUID generado por el cliente es la llave;
   usar `ON CONFLICT` / verificación de estado).
6. **Nombres en español** en tablas, columnas, structs y endpoints.
7. **Borrado físico prohibido en maestros** (productos, clientes, usuarios,
   proveedores, categorías): desactivación con `activo = false`, auditada.
8. **Todo hecho de negocio lleva `usuario_id`** (quién lo hizo).
9. Tests obligatorios para: cálculo de precio final, transacción de
   confirmación de recepción, asignación FEFO, idempotencia de sincronización
   de ventas, verificación de permisos.

## 3. CONTEXTO: CATÁLOGO (schema `catalogo`)

- **categorias**: nombre único, `padre_id` opcional (jerarquía solo
  organizativa), `markup_pct` (default 40.00), `iva_pct` (default 21.00).
  **La herencia de markup/IVA es SOLO desde la categoría directa del producto;
  nunca se sube por el árbol.**
- **productos**: nombre, `categoria_id`, `markup_pct_override` (nullable),
  `iva_pct_override` (nullable — en Argentina el IVA es por producto: 21%,
  10.5%, exento), `unidad_de_venta` enum (`unidad` | `peso`),
  `controla_vencimiento` bool (default false), `precio_actual_centavos` y
  `costo_actual_centavos` (proyecciones), `activo`.
- **codigos_barras**: N por producto (`codigo` PK texto, `producto_id`,
  descripción opcional). Es el hot path del escaneo.
- **precios_historial** (ledger): producto, precio, costo, `recepcion_id`
  nullable (NULL = cambio manual), `usuario_id`, `vigente_desde`.
- Índice de trigramas (`pg_trgm`) sobre `productos.nombre` para el buscador
  con autocompletado tolerante a errores de tipeo.
- Cascada de resolución de markup/IVA: valor explícito en el documento →
  override del producto → default de la categoría.

## 4. CONTEXTO: COMPRAS (schema `compras`) — el corazón del sistema

- **proveedores**: nombre, cuit, teléfono, `precios_con_iva` bool (default del
  toggle al cargar sus recepciones), condiciones de pago (texto libre), activo.
- **recepciones**: proveedor nullable, estado (`borrador` → `confirmada` →
  `completada`), observaciones, timestamps de cada transición.
- **recepcion_items**: recepción, producto, **cantidad** (NUMERIC),
  `costo_centavos`, `costo_incluye_iva`, `iva_pct`, `markup_pct`,
  `precio_final_centavos` — **todos snapshot** (si mañana cambia el markup de
  la categoría, este documento sigue reflejando lo aplicado),
  `vencimiento` (DATE nullable — **obligatorio si el producto tiene
  `controla_vencimiento`**), `etiquetado` bool + `etiquetado_en`.
  UNIQUE (recepcion, producto); carga idempotente con ON CONFLICT DO UPDATE.
- **Cálculo de precio final**:
  `base = costo` (si incluye IVA) o `costo × (1 + iva/100)` (si no);
  `precio_final = base × (1 + markup/100)`, redondeado a centavos al final.
- **LA TRANSACCIÓN CRÍTICA — confirmar recepción** (atómica, con lock
  `FOR UPDATE` sobre la recepción, idempotente si ya está confirmada):
  1. Por cada ítem: INSERT en `precios_historial` + UPDATE de proyecciones de
     precio/costo en el producto.
  2. Por cada ítem: INSERT de movimiento `entrada_recepcion` en el ledger de
     inventario (al depósito principal); si el producto controla vencimiento,
     crear el lote de inventario con su fecha y asociar el movimiento.
  3. Marcar recepción como `confirmada`. Los ítems quedan con
     `etiquetado = false` (trabajo pendiente para el recorrido de etiquetado).
  Todo o nada. Nunca una recepción a medias aplicada.
- **El historial de costos NO tiene tabla propia**: cada `recepcion_item` es un
  registro de costo con fecha y proveedor. Costo actual = último recibido.
- Endpoints del flujo de etiquetado: listar ítems pendientes de una recepción,
  marcar ítem etiquetado (idempotente; al no quedar pendientes, la recepción
  pasa a `completada`). Estos endpoints los consume la PWA durante el
  recorrido físico con impresora térmica Bluetooth.

## 5. CONTEXTO: INVENTARIO (schema `inventario`)

- **depositos**: sembrar uno ("Principal"). `deposito_id` es NOT NULL en
  movimientos y proyecciones desde el día cero, aunque hoy haya uno solo.
  Las features multi-depósito (transferencias, permisos) NO se construyen.
- **lotes**: producto, código de lote del proveedor (opcional), `vencimiento`,
  `recepcion_item_id` de origen, `cantidad_actual` (proyección).
- **movimientos_stock** (ledger, solo-INSERT): producto, depósito, `lote_id`
  nullable, **cantidad NUMERIC con signo** (entradas +, salidas −; el stock es
  literalmente SUM(cantidad)), tipo enum de exactamente 5 valores:
  `entrada_recepcion`, `salida_venta`, `devolucion_cliente`,
  `devolucion_proveedor`, `ajuste`. Origen por **FKs explícitas nullable**
  (`recepcion_item_id`, `venta_item_id`, `ajuste_id`) con CHECK de que
  exactamente una esté presente y sea coherente con el tipo. NO usar
  referencia polimórfica genérica. `usuario_id`, timestamp.
- **ajustes** (documento): motivo (`perdida` | `rotura` | `vencimiento` |
  `robo` | `conteo` | `otro`), observaciones, usuario. **Robo, pérdida y
  vencimiento son MOTIVOS del ajuste, no tipos de movimiento**: el robo se
  descubre como faltante en conteos, no se registra en el momento.
- **stock_actual** (proyección): producto + depósito → cantidad.
- **FEFO por asunción**: al sincronizar una venta, el servidor descuenta del
  lote con vencimiento más próximo que tenga stock (solo para productos con
  lotes). NUNCA se pide selección de lote en la caja. Es una aproximación
  deliberada; el conteo físico la recalibra vía ajuste. Los lotes existen para
  alertas de vencimiento accionables, no para trazabilidad exacta.
- **Stock negativo PERMITIDO en ventas** (decisión de negocio: la caja jamás
  bloquea con el cliente en el mostrador; el error es del sistema, no del
  cliente). Sin CHECK de stock ≥ 0. Ajustes y devoluciones a proveedor sí
  pueden validar disponibilidad.

## 6. CONTEXTO: VENTAS/CAJA (schema `ventas`)

- **sesiones_caja**: usuario, monto inicial, `abierta_en`, `cerrada_en`,
  monto contado al cierre, **diferencia de arqueo registrada, nunca corregida
  silenciosamente**. Toda venta pertenece a una sesión.
- **ventas**: UUID **generado en el dispositivo** (offline-first), sesión,
  cliente nullable, totales, descuento de ticket (monto + motivo), estado
  (`confirmada` | `anulada`), timestamp del dispositivo y de sincronización.
- **venta_items**: FK a producto **+ snapshot de nombre y precio al momento de
  la venta** (los documentos históricos son autocontenidos; el precio del
  producto cambia todas las semanas), cantidad decimal, `iva_pct` snapshot,
  descuento de línea (monto + motivo).
- **pagos** (tabla hija, N por venta): medio (`efectivo` | `tarjeta` |
  `mercado_pago` | `transferencia` | `cuenta_corriente`), monto,
  `referencia_externa` nullable. **Invariante verificado al confirmar:
  Σ pagos = total.** El pago con `cuenta_corriente` inserta un cargo en el
  ledger de Clientes (misma transacción) referenciando la venta.
- **Sincronización**: la venta llega ya confirmada desde el dispositivo. El
  servidor la persiste (idempotente por UUID: reintento = no-op) y **genera él
  los movimientos de stock** (la caja no sabe de lotes ni FEFO; un solo
  escritor del ledger por tipo de documento).
- **Anulación**: nueva acción con permiso, estado `anulada` + movimientos de
  stock inversos referenciando los originales. Jamás editar una venta.
- Productos por peso: la interpretación del código de balanza (prefijo 2 con
  peso embebido) es lógica del cliente/caja; el backend solo recibe
  `cantidad = 0.475`. Nada especial que construir.
- Preparación fiscal (NO construir la integración): el comprobante fiscal será
  una entidad futura separada que referencia la venta. Hoy solo garantizar que
  el documento de venta tenga IVA por ítem (lo tiene) e identificación de
  cliente nullable.

## 7. CONTEXTO: CLIENTES (schema `clientes`)

- **clientes**: nombre (único campo obligatorio), teléfono, documento,
  `limite_credito_centavos` nullable (NULL = sin límite), `saldo_actual`
  (proyección), activo.
- **cuenta_movimientos** (ledger): tipo (`cargo` | `pago` | `ajuste`), monto
  con signo, `venta_id` nullable (los cargos referencian la venta — la cuenta
  NO almacena productos ni duplica ventas), medio de pago (para pagos),
  motivo (para ajustes), usuario, timestamp.
- **Saldo global corrido, SIN imputación de pagos contra ventas específicas**
  (es una libreta de fiado, no contabilidad formal; la imputación sería una
  tabla futura de asignaciones si algún día hace falta).
- **El límite de crédito SÍ bloquea** (asimetría deliberada con el stock
  negativo: esto es riesgo de plata decidido por el dueño, no un error del
  sistema); excederlo requiere el permiso `exceder_limite_credito`.

## 8. CONTEXTO: IDENTIDAD (schema `identidad`)

- **Permisos: catálogo FIJO definido en código** (constantes versionadas con
  el software), NUNCA creables desde la UI. Mínimo inicial: `vender`,
  `anular_venta`, `aplicar_descuento`, `exceder_limite_credito`,
  `confirmar_recepcion`, `ajustar_stock`, `modificar_precios`,
  `gestionar_usuarios`, `gestionar_clientes`, `ver_reportes`, `cerrar_caja`,
  `abrir_caja`.
- **roles**: bundles de permisos, creables y editables libremente por el
  administrador. Sembrar como punto de partida: Administrador (todos),
  Encargado, Cajero, Repositor. **La jerarquía NO se modela** — es plana:
  usuario → rol → permisos, fin de la traza. Sin herencia entre roles.
- **usuarios**: nombre, `password_hash` (acceso administrativo),
  `pin_hash` (PIN corto de 4-6 dígitos para cambio rápido de operador en la
  caja compartida), rol, permisos individuales adicionales opcionales
  (aditivos), activo.
- **Denegado por defecto. SIN permisos negativos/deny-overrides** (si hay que
  quitarle algo a alguien, su rol está mal armado).

## 9. AUDITORÍA (schema `auditoria`)

- Los ledgers YA auditan precios, stock, ventas y cuentas corrientes. NO
  duplicar eso.
- **auditoria_eventos**: entidad, entidad_id, acción, usuario, timestamp,
  diff JSONB. Registra SOLO: mutaciones de datos maestros (productos,
  categorías, proveedores, clientes, usuarios, roles) y acciones de seguridad
  (cambios de permisos, desactivaciones, logins administrativos fallidos).
- No se auditan lecturas.

## 10. FASES DE CONSTRUCCIÓN (en este orden, sin adelantarse)

- **Fase 1**: Identidad mínima (usuarios, PIN, roles sembrados, middleware de
  permisos) + Catálogo completo + Compras completo con el flujo de etiquetado.
  Esta fase resuelve el dolor original del negocio (actualización de precios y
  etiquetas al recibir mercadería) y debe ser usable por sí sola.
- **Fase 2**: Inventario (ledger, lotes, ajustes, alertas de vencimiento,
  stock en pantalla de producto).
- **Fase 3**: Ventas/Caja (sesiones, ventas, pagos, sincronización idempotente,
  anulaciones, generación server-side de movimientos).
- **Fase 4**: Clientes y cuenta corriente.
- Auditoría transversal se implementa desde la Fase 1 (es un middleware/helper,
  no un módulo final).

## 11. EXCLUSIONES EXPLÍCITAS — NO CONSTRUIR (ni "dejar preparado" con código)

Órdenes de compra; motor de promociones (los campos de descuento ya aceptan un
origen futuro); integración fiscal AFIP/ARCA; features multi-sucursal y
multi-depósito (solo existe la columna `deposito_id`); transferencias entre
depósitos; CRM y fidelización; imputación de pagos; tienda online; integración
Mercado Pago (solo el campo `referencia_externa`); listas de precios múltiples;
costeo promedio ponderado; jerarquía de roles; deny-overrides; 2FA/OAuth;
microservicios; motor de reportes (endpoints de consulta simples están bien).
Todas estas son extensiones aditivas ya verificadas contra el diseño: entran
después sin reescribir nada. Construirlas hoy es especulación.

## 12. FORMA DE TRABAJO

- Avanzá fase por fase; dentro de cada fase, migraciones primero, luego
  dominio, luego endpoints, luego tests.
- Ante cualquier ambigüedad o contradicción con este documento: preguntá,
  no asumas.
- Este documento es la fuente de verdad arquitectónica del proyecto. Mantenelo
  en el repo como `ARQUITECTURA.md` y actualizalo (con el usuario) si una
  decisión cambia.
