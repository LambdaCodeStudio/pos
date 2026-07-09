-- AUDITORÍA transversal. Los ledgers ya auditan precios, stock, ventas y
-- cuentas corrientes — acá solo van mutaciones de datos maestros y acciones
-- de seguridad. No se auditan lecturas.

CREATE TABLE auditoria.auditoria_eventos (
    id         UUID PRIMARY KEY,
    entidad    TEXT NOT NULL,
    -- NULL cuando no hay entidad concreta (p. ej. login fallido de un nombre
    -- de usuario inexistente).
    entidad_id UUID,
    accion     TEXT NOT NULL,
    usuario_id UUID REFERENCES identidad.usuarios(id),
    diff       JSONB,
    creado_en  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX auditoria_eventos_entidad_idx
    ON auditoria.auditoria_eventos (entidad, entidad_id);
CREATE INDEX auditoria_eventos_creado_idx
    ON auditoria.auditoria_eventos (creado_en DESC);
