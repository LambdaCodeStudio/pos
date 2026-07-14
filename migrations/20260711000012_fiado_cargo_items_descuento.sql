-- Fase 6: el FIFO de fiado y el reprecio automático valuaban siempre al
-- precio de catálogo vigente, ignorando que la venta original pudo tener un
-- descuento de ticket. factor_descuento guarda la proporción realmente
-- cobrada (total_centavos / Σ subtotales de la venta) y se pondera junto al
-- precio corriente en cada valuación, sin perder el diseño de "revalúa a
-- precio corriente" (ver 20260709000011_fiado_cargo_items.sql).

ALTER TABLE clientes.cargo_items
    ADD COLUMN factor_descuento NUMERIC(9,6) NOT NULL DEFAULT 1
    CHECK (factor_descuento > 0 AND factor_descuento <= 1);
