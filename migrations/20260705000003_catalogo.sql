-- Contexto CATÁLOGO: categorías, productos, códigos de barras, historial de precios.

CREATE TABLE catalogo.categorias (
    id             UUID PRIMARY KEY,
    nombre         TEXT NOT NULL UNIQUE,
    -- Jerarquía solo organizativa: la herencia de markup/IVA es SOLO desde la
    -- categoría directa del producto, nunca se sube por el árbol.
    padre_id       UUID REFERENCES catalogo.categorias(id),
    markup_pct     NUMERIC(5,2) NOT NULL DEFAULT 40.00,
    iva_pct        NUMERIC(5,2) NOT NULL DEFAULT 21.00,
    activo         BOOLEAN NOT NULL DEFAULT true,
    creado_en      TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TYPE catalogo.unidad_de_venta AS ENUM ('unidad', 'peso');

CREATE TABLE catalogo.productos (
    id                     UUID PRIMARY KEY,
    nombre                 TEXT NOT NULL,
    categoria_id           UUID NOT NULL REFERENCES catalogo.categorias(id),
    markup_pct_override    NUMERIC(5,2),
    iva_pct_override       NUMERIC(5,2),
    unidad_de_venta        catalogo.unidad_de_venta NOT NULL DEFAULT 'unidad',
    controla_vencimiento   BOOLEAN NOT NULL DEFAULT false,
    -- Proyecciones (fuente de verdad: catalogo.precios_historial).
    precio_actual_centavos BIGINT,
    costo_actual_centavos  BIGINT,
    activo                 BOOLEAN NOT NULL DEFAULT true,
    creado_en              TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX productos_nombre_trgm_idx ON catalogo.productos USING gin (nombre gin_trgm_ops);
CREATE INDEX productos_categoria_idx ON catalogo.productos (categoria_id);

-- Hot path del escaneo en caja: PK sobre el código.
CREATE TABLE catalogo.codigos_barras (
    codigo      TEXT PRIMARY KEY,
    producto_id UUID NOT NULL REFERENCES catalogo.productos(id),
    descripcion TEXT,
    creado_en   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX codigos_barras_producto_idx ON catalogo.codigos_barras (producto_id);

-- Ledger de precios: solo-INSERT. recepcion_id NULL = cambio manual.
-- La FK a compras.recepciones se agrega en la migración de compras.
CREATE TABLE catalogo.precios_historial (
    id              UUID PRIMARY KEY,
    producto_id     UUID NOT NULL REFERENCES catalogo.productos(id),
    precio_centavos BIGINT NOT NULL,
    costo_centavos  BIGINT,
    recepcion_id    UUID,
    usuario_id      UUID NOT NULL REFERENCES identidad.usuarios(id),
    vigente_desde   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX precios_historial_producto_idx
    ON catalogo.precios_historial (producto_id, vigente_desde DESC);
