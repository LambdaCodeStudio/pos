-- Contexto INVENTARIO — base requerida por la transacción de confirmación de
-- recepción (Fase 1). Ajustes, alertas de vencimiento y endpoints propios
-- llegan en la Fase 2.

CREATE TABLE inventario.depositos (
    id        UUID PRIMARY KEY,
    nombre    TEXT NOT NULL UNIQUE,
    activo    BOOLEAN NOT NULL DEFAULT true,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Depósito único de hoy; deposito_id es NOT NULL en todo desde el día cero.
INSERT INTO inventario.depositos (id, nombre)
VALUES ('01900000-0000-7000-8000-00000000d001', 'Principal');

-- Lotes: existen para alertas de vencimiento accionables (FEFO por asunción),
-- no para trazabilidad exacta.
CREATE TABLE inventario.lotes (
    id                UUID PRIMARY KEY,
    producto_id       UUID NOT NULL REFERENCES catalogo.productos(id),
    codigo_lote       TEXT,
    vencimiento       DATE NOT NULL,
    recepcion_item_id UUID REFERENCES compras.recepcion_items(id),
    -- Proyección (fuente de verdad: movimientos_stock con este lote_id).
    cantidad_actual   NUMERIC(12,3) NOT NULL DEFAULT 0,
    creado_en         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX lotes_producto_vencimiento_idx ON inventario.lotes (producto_id, vencimiento);

CREATE TYPE inventario.tipo_movimiento AS ENUM (
    'entrada_recepcion',
    'salida_venta',
    'devolucion_cliente',
    'devolucion_proveedor',
    'ajuste'
);

-- Ledger solo-INSERT. El stock es literalmente SUM(cantidad).
-- Correcciones = contra-asientos, nunca UPDATE/DELETE.
CREATE TABLE inventario.movimientos_stock (
    id                UUID PRIMARY KEY,
    producto_id       UUID NOT NULL REFERENCES catalogo.productos(id),
    deposito_id       UUID NOT NULL REFERENCES inventario.depositos(id),
    lote_id           UUID REFERENCES inventario.lotes(id),
    -- Con signo: entradas +, salidas −.
    cantidad          NUMERIC(12,3) NOT NULL CHECK (cantidad <> 0),
    tipo              inventario.tipo_movimiento NOT NULL,
    -- Origen por FKs explícitas, exactamente una y coherente con el tipo.
    -- venta_item_id y ajuste_id reciben su FK en las fases 3 y 2.
    recepcion_item_id UUID REFERENCES compras.recepcion_items(id),
    venta_item_id     UUID,
    ajuste_id         UUID,
    usuario_id        UUID NOT NULL REFERENCES identidad.usuarios(id),
    creado_en         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT movimientos_stock_un_origen CHECK (
        (recepcion_item_id IS NOT NULL)::int
        + (venta_item_id IS NOT NULL)::int
        + (ajuste_id IS NOT NULL)::int = 1
    ),
    CONSTRAINT movimientos_stock_origen_coherente CHECK (
        (tipo = 'entrada_recepcion'    AND recepcion_item_id IS NOT NULL)
        OR (tipo = 'salida_venta'         AND venta_item_id IS NOT NULL)
        OR (tipo = 'devolucion_cliente'   AND venta_item_id IS NOT NULL)
        OR (tipo = 'devolucion_proveedor' AND recepcion_item_id IS NOT NULL)
        OR (tipo = 'ajuste'               AND ajuste_id IS NOT NULL)
    )
);

CREATE INDEX movimientos_stock_producto_idx
    ON inventario.movimientos_stock (producto_id, deposito_id, creado_en DESC);
CREATE INDEX movimientos_stock_lote_idx
    ON inventario.movimientos_stock (lote_id) WHERE lote_id IS NOT NULL;

-- Proyección de stock. Sin CHECK de cantidad >= 0: el stock negativo está
-- PERMITIDO en ventas (la caja jamás bloquea con el cliente en el mostrador).
CREATE TABLE inventario.stock_actual (
    producto_id    UUID NOT NULL REFERENCES catalogo.productos(id),
    deposito_id    UUID NOT NULL REFERENCES inventario.depositos(id),
    cantidad       NUMERIC(12,3) NOT NULL DEFAULT 0,
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (producto_id, deposito_id)
);
