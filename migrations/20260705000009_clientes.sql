-- Fase 4: contexto CLIENTES — la libreta de fiado.
-- Saldo global corrido, SIN imputación de pagos contra ventas específicas.
-- Convención de signo del ledger: positivo = el cliente debe más (cargo),
-- negativo = debe menos (pago). saldo_actual = SUM(monto).

CREATE SCHEMA IF NOT EXISTS clientes;

CREATE TABLE clientes.clientes (
    id                      UUID PRIMARY KEY,
    -- Único campo obligatorio (puede haber dos "Juan Pérez": no es UNIQUE).
    nombre                  TEXT NOT NULL,
    telefono                TEXT,
    documento               TEXT,
    -- NULL = sin límite. El límite SÍ bloquea (asimetría deliberada con el
    -- stock negativo: esto es riesgo de plata decidido por el dueño).
    limite_credito_centavos BIGINT CHECK (limite_credito_centavos >= 0),
    -- Proyección (fuente de verdad: clientes.cuenta_movimientos).
    saldo_actual_centavos   BIGINT NOT NULL DEFAULT 0,
    activo                  BOOLEAN NOT NULL DEFAULT true,
    creado_en               TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX clientes_nombre_idx ON clientes.clientes (nombre);

CREATE TYPE clientes.tipo_movimiento_cuenta AS ENUM ('cargo', 'pago', 'ajuste');

-- Ledger solo-INSERT. Los cargos referencian la venta: la cuenta NO almacena
-- productos ni duplica ventas. Correcciones = contra-asientos (tipo ajuste).
CREATE TABLE clientes.cuenta_movimientos (
    id             UUID PRIMARY KEY,
    cliente_id     UUID NOT NULL REFERENCES clientes.clientes(id),
    tipo           clientes.tipo_movimiento_cuenta NOT NULL,
    monto_centavos BIGINT NOT NULL CHECK (monto_centavos <> 0),
    venta_id       UUID REFERENCES ventas.ventas(id),
    -- Con qué pagó (para tipo pago).
    medio_pago     ventas.medio_pago,
    -- Por qué se ajusta (para tipo ajuste).
    motivo         TEXT,
    usuario_id     UUID NOT NULL REFERENCES identidad.usuarios(id),
    creado_en      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT cuenta_movimientos_coherente CHECK (
        (tipo = 'cargo'  AND venta_id IS NOT NULL AND monto_centavos > 0)
        OR (tipo = 'pago'   AND medio_pago IS NOT NULL AND monto_centavos < 0)
        OR (tipo = 'ajuste' AND motivo IS NOT NULL)
    )
);

CREATE INDEX cuenta_movimientos_cliente_idx
    ON clientes.cuenta_movimientos (cliente_id, creado_en DESC);
CREATE INDEX cuenta_movimientos_venta_idx
    ON clientes.cuenta_movimientos (venta_id) WHERE venta_id IS NOT NULL;

-- La columna existía desde la Fase 3 esperando esta tabla.
ALTER TABLE ventas.ventas
    ADD CONSTRAINT ventas_cliente_fk
    FOREIGN KEY (cliente_id) REFERENCES clientes.clientes(id);
