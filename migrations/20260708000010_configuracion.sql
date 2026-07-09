-- Configuración global del negocio: una fila por parámetro, valor JSONB.

CREATE TABLE catalogo.configuracion (
    clave          TEXT PRIMARY KEY,
    valor          JSONB NOT NULL,
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Redondeo comercial del precio de venta calculado en recepciones, en
-- centavos: 0 = sin redondeo; 10000 = al múltiplo de $100 más cercano
-- (resto < $50 baja, >= $50 sube). No afecta precios ya vigentes.
INSERT INTO catalogo.configuracion (clave, valor)
VALUES ('redondeo_precio_centavos', '0');
