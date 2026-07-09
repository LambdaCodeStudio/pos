-- Fase 2: documento de ajuste de inventario.
-- Robo, pérdida y vencimiento son MOTIVOS del ajuste, no tipos de movimiento:
-- el robo se descubre como faltante en conteos, no se registra en el momento.

CREATE TYPE inventario.motivo_ajuste AS ENUM (
    'perdida',
    'rotura',
    'vencimiento',
    'robo',
    'conteo',
    'otro'
);

CREATE TABLE inventario.ajustes (
    id            UUID PRIMARY KEY,
    motivo        inventario.motivo_ajuste NOT NULL,
    observaciones TEXT,
    usuario_id    UUID NOT NULL REFERENCES identidad.usuarios(id),
    creado_en     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- La columna existía desde la Fase 1 esperando esta tabla.
ALTER TABLE inventario.movimientos_stock
    ADD CONSTRAINT movimientos_stock_ajuste_fk
    FOREIGN KEY (ajuste_id) REFERENCES inventario.ajustes(id);
