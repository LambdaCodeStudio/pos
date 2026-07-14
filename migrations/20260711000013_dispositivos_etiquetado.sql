-- Contexto IDENTIDAD: dispositivos de campo (etiquetadora ESP32), autenticados
-- por secreto HMAC compartido en lugar de usuario/rol. El secreto se guarda
-- en TEXTO CLARO, NO hasheado: el servidor necesita el valor real para
-- recalcular el HMAC de cada request (esquema de secreto compartido). La
-- protección acá es de acceso a la tabla, no de hashing — no "mejorar" esto
-- hasheando el secreto, rompería la verificación.
CREATE TABLE identidad.dispositivos (
    id             UUID PRIMARY KEY,
    device_id      TEXT NOT NULL UNIQUE,
    secreto_hmac   TEXT NOT NULL,
    descripcion    TEXT,
    activo         BOOLEAN NOT NULL DEFAULT true,
    creado_en      TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Atribución del flag de etiquetado al dispositivo que lo escaneó (paralelo a
-- etiquetado_por, que queda para una eventual marca manual con usuario).
ALTER TABLE compras.recepcion_items
    ADD COLUMN etiquetado_por_dispositivo_id UUID REFERENCES identidad.dispositivos(id);
