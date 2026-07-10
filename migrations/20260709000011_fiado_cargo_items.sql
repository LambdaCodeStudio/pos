-- Fase 5: fiado indexado a producto.
-- Cada cargo (venta fiada) se descompone en renglones por producto/cantidad.
-- El saldo pendiente de cada renglón se cancela por FIFO (el pago más nuevo
-- salda primero el cargo más viejo) a precio corriente, y se revalúa —para
-- arriba o para abajo— cuando el producto cambia de precio mientras el
-- renglón siga pendiente. Es una proyección mutable (igual que
-- saldo_actual_centavos): el ledger clientes.cuenta_movimientos sigue
-- siendo solo-INSERT; acá solo se actualiza cantidad_pendiente.

CREATE TABLE clientes.cargo_items (
    id                 UUID PRIMARY KEY,
    movimiento_id      UUID NOT NULL REFERENCES clientes.cuenta_movimientos(id),
    cliente_id         UUID NOT NULL REFERENCES clientes.clientes(id),
    producto_id        UUID NOT NULL REFERENCES catalogo.productos(id),
    -- Snapshot: si el producto se renombra después, el renglón conserva el
    -- nombre con el que se fió.
    producto_nombre    TEXT NOT NULL,
    cantidad           NUMERIC(12,3) NOT NULL CHECK (cantidad > 0),
    -- Cantidad todavía no saldada por ningún pago. El reprecio NUNCA toca
    -- esta columna (solo cambia el valor en pesos); los pagos SÍ, vía FIFO.
    cantidad_pendiente NUMERIC(12,3) NOT NULL CHECK (cantidad_pendiente >= 0),
    creado_en          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT cargo_items_pendiente_no_excede CHECK (cantidad_pendiente <= cantidad)
);

CREATE INDEX cargo_items_cliente_pendiente_idx
    ON clientes.cargo_items (cliente_id, creado_en)
    WHERE cantidad_pendiente > 0;
CREATE INDEX cargo_items_producto_pendiente_idx
    ON clientes.cargo_items (producto_id)
    WHERE cantidad_pendiente > 0;
CREATE INDEX cargo_items_movimiento_idx ON clientes.cargo_items (movimiento_id);

-- Auditoría de qué pago (o condonación) saldó qué renglón y a qué valor.
-- clientes.cuenta_movimientos ya tiene el monto total del pago; esto es
-- la traza de cómo se repartió entre renglones pendientes.
CREATE TABLE clientes.cargo_aplicaciones (
    id                      UUID PRIMARY KEY,
    pago_movimiento_id      UUID NOT NULL REFERENCES clientes.cuenta_movimientos(id),
    cargo_item_id           UUID NOT NULL REFERENCES clientes.cargo_items(id),
    cantidad_aplicada       NUMERIC(12,3) NOT NULL CHECK (cantidad_aplicada > 0),
    valor_centavos_aplicado BIGINT NOT NULL CHECK (valor_centavos_aplicado > 0),
    creado_en               TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX cargo_aplicaciones_pago_idx ON clientes.cargo_aplicaciones (pago_movimiento_id);
CREATE INDEX cargo_aplicaciones_cargo_item_idx ON clientes.cargo_aplicaciones (cargo_item_id);
