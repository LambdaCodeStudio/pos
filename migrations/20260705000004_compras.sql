-- Contexto COMPRAS: proveedores, recepciones y sus ítems.
-- Cada recepcion_item ES el historial de costos (no hay tabla aparte):
-- todos los valores de cálculo son snapshot del momento de la carga.

CREATE TABLE compras.proveedores (
    id               UUID PRIMARY KEY,
    nombre           TEXT NOT NULL,
    cuit             TEXT,
    telefono         TEXT,
    -- Default del toggle "costo incluye IVA" al cargar sus recepciones.
    precios_con_iva  BOOLEAN NOT NULL DEFAULT true,
    condiciones_pago TEXT,
    activo           BOOLEAN NOT NULL DEFAULT true,
    creado_en        TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TYPE compras.estado_recepcion AS ENUM ('borrador', 'confirmada', 'completada');

CREATE TABLE compras.recepciones (
    id             UUID PRIMARY KEY,
    proveedor_id   UUID REFERENCES compras.proveedores(id),
    estado         compras.estado_recepcion NOT NULL DEFAULT 'borrador',
    observaciones  TEXT,
    usuario_id     UUID NOT NULL REFERENCES identidad.usuarios(id),
    creada_en      TIMESTAMPTZ NOT NULL DEFAULT now(),
    confirmada_en  TIMESTAMPTZ,
    confirmada_por UUID REFERENCES identidad.usuarios(id),
    completada_en  TIMESTAMPTZ
);

CREATE TABLE compras.recepcion_items (
    id                    UUID PRIMARY KEY,
    recepcion_id          UUID NOT NULL REFERENCES compras.recepciones(id),
    producto_id           UUID NOT NULL REFERENCES catalogo.productos(id),
    cantidad              NUMERIC(12,3) NOT NULL CHECK (cantidad > 0),
    -- Snapshot completo del cálculo: si mañana cambia el markup de la
    -- categoría, este documento sigue reflejando lo aplicado.
    costo_centavos        BIGINT NOT NULL CHECK (costo_centavos >= 0),
    costo_incluye_iva     BOOLEAN NOT NULL,
    iva_pct               NUMERIC(5,2) NOT NULL,
    markup_pct            NUMERIC(5,2) NOT NULL,
    precio_final_centavos BIGINT NOT NULL CHECK (precio_final_centavos >= 0),
    -- Obligatorio si el producto tiene controla_vencimiento (validado en dominio).
    vencimiento           DATE,
    etiquetado            BOOLEAN NOT NULL DEFAULT false,
    etiquetado_en         TIMESTAMPTZ,
    etiquetado_por        UUID REFERENCES identidad.usuarios(id),
    usuario_id            UUID NOT NULL REFERENCES identidad.usuarios(id),
    creado_en             TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Carga idempotente: ON CONFLICT (recepcion_id, producto_id) DO UPDATE.
    UNIQUE (recepcion_id, producto_id)
);

CREATE INDEX recepcion_items_recepcion_idx ON compras.recepcion_items (recepcion_id);
CREATE INDEX recepcion_items_producto_idx ON compras.recepcion_items (producto_id);

ALTER TABLE catalogo.precios_historial
    ADD CONSTRAINT precios_historial_recepcion_fk
    FOREIGN KEY (recepcion_id) REFERENCES compras.recepciones(id);
