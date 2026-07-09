-- Fase 3: contexto VENTAS/CAJA.
-- Las ventas nacen en el dispositivo (UUID del cliente, offline-first) y el
-- servidor las persiste idempotentemente y genera él los movimientos de stock.

CREATE SCHEMA IF NOT EXISTS ventas;

CREATE TABLE ventas.sesiones_caja (
    id                         UUID PRIMARY KEY,
    usuario_id                 UUID NOT NULL REFERENCES identidad.usuarios(id),
    monto_inicial_centavos     BIGINT NOT NULL CHECK (monto_inicial_centavos >= 0),
    abierta_en                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    cerrada_en                 TIMESTAMPTZ,
    -- Arqueo: lo contado se registra tal cual; la diferencia queda asentada,
    -- NUNCA se corrige silenciosamente.
    monto_contado_centavos     BIGINT,
    diferencia_arqueo_centavos BIGINT,
    CHECK ((cerrada_en IS NULL) = (monto_contado_centavos IS NULL))
);

CREATE INDEX sesiones_caja_usuario_idx ON ventas.sesiones_caja (usuario_id, abierta_en DESC);

CREATE TYPE ventas.estado_venta AS ENUM ('confirmada', 'anulada');

CREATE TABLE ventas.ventas (
    id               UUID PRIMARY KEY,  -- generado en el dispositivo
    sesion_id        UUID NOT NULL REFERENCES ventas.sesiones_caja(id),
    -- FK a clientes.clientes se agrega en la Fase 4.
    cliente_id       UUID,
    total_centavos   BIGINT NOT NULL CHECK (total_centavos >= 0),
    -- Descuento de ticket: monto + motivo (el motivo permite un origen
    -- futuro de promociones sin motor de promociones hoy).
    descuento_centavos BIGINT NOT NULL DEFAULT 0 CHECK (descuento_centavos >= 0),
    descuento_motivo TEXT,
    estado           ventas.estado_venta NOT NULL DEFAULT 'confirmada',
    usuario_id       UUID NOT NULL REFERENCES identidad.usuarios(id),
    vendida_en       TIMESTAMPTZ NOT NULL,               -- reloj del dispositivo
    sincronizada_en  TIMESTAMPTZ NOT NULL DEFAULT now(), -- reloj del servidor
    anulada_en       TIMESTAMPTZ,
    anulada_por      UUID REFERENCES identidad.usuarios(id),
    anulacion_motivo TEXT
);

CREATE INDEX ventas_sesion_idx ON ventas.ventas (sesion_id);
CREATE INDEX ventas_vendida_idx ON ventas.ventas (vendida_en DESC);

-- Documento histórico autocontenido: snapshot de nombre, precio e IVA al
-- momento de la venta (el precio cambia todas las semanas).
CREATE TABLE ventas.venta_items (
    id                       UUID PRIMARY KEY,
    venta_id                 UUID NOT NULL REFERENCES ventas.ventas(id),
    producto_id              UUID NOT NULL REFERENCES catalogo.productos(id),
    producto_nombre          TEXT NOT NULL,
    precio_unitario_centavos BIGINT NOT NULL CHECK (precio_unitario_centavos >= 0),
    cantidad                 NUMERIC(12,3) NOT NULL CHECK (cantidad > 0),
    iva_pct                  NUMERIC(5,2) NOT NULL,
    descuento_centavos       BIGINT NOT NULL DEFAULT 0 CHECK (descuento_centavos >= 0),
    descuento_motivo         TEXT,
    subtotal_centavos        BIGINT NOT NULL CHECK (subtotal_centavos >= 0)
);

CREATE INDEX venta_items_venta_idx ON ventas.venta_items (venta_id);
CREATE INDEX venta_items_producto_idx ON ventas.venta_items (producto_id);

CREATE TYPE ventas.medio_pago AS ENUM (
    'efectivo', 'tarjeta', 'mercado_pago', 'transferencia', 'cuenta_corriente'
);

-- N pagos por venta. Invariante verificado al sincronizar: Σ pagos = total.
CREATE TABLE ventas.pagos (
    id                 UUID PRIMARY KEY,
    venta_id           UUID NOT NULL REFERENCES ventas.ventas(id),
    medio              ventas.medio_pago NOT NULL,
    monto_centavos     BIGINT NOT NULL CHECK (monto_centavos > 0),
    referencia_externa TEXT
);

CREATE INDEX pagos_venta_idx ON ventas.pagos (venta_id);

-- La columna existía desde la Fase 1 esperando esta tabla.
ALTER TABLE inventario.movimientos_stock
    ADD CONSTRAINT movimientos_stock_venta_item_fk
    FOREIGN KEY (venta_item_id) REFERENCES ventas.venta_items(id);
