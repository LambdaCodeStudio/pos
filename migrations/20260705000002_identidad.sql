-- Contexto IDENTIDAD: usuarios, roles y permisos.
-- Los permisos son un catálogo FIJO definido en código (src/identidad/permisos.rs);
-- la base solo almacena sus nombres. Modelo plano: usuario -> rol -> permisos,
-- más permisos individuales aditivos. Sin jerarquía de roles, sin deny-overrides.

CREATE TABLE identidad.roles (
    id            UUID PRIMARY KEY,
    nombre        TEXT NOT NULL UNIQUE,
    descripcion   TEXT,
    activo        BOOLEAN NOT NULL DEFAULT true,
    creado_en     TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE identidad.rol_permisos (
    rol_id   UUID NOT NULL REFERENCES identidad.roles(id) ON DELETE CASCADE,
    permiso  TEXT NOT NULL,
    PRIMARY KEY (rol_id, permiso)
);

CREATE TABLE identidad.usuarios (
    id             UUID PRIMARY KEY,
    nombre         TEXT NOT NULL UNIQUE,
    password_hash  TEXT NOT NULL,
    pin_hash       TEXT,
    rol_id         UUID NOT NULL REFERENCES identidad.roles(id),
    activo         BOOLEAN NOT NULL DEFAULT true,
    creado_en      TIMESTAMPTZ NOT NULL DEFAULT now(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Permisos individuales adicionales (solo aditivos).
CREATE TABLE identidad.usuario_permisos (
    usuario_id UUID NOT NULL REFERENCES identidad.usuarios(id) ON DELETE CASCADE,
    permiso    TEXT NOT NULL,
    PRIMARY KEY (usuario_id, permiso)
);

-- Roles sembrados como punto de partida (editables por el administrador).
-- UUIDs fijos para que los seeds sean deterministas.
INSERT INTO identidad.roles (id, nombre, descripcion) VALUES
    ('01900000-0000-7000-8000-000000000001', 'Administrador', 'Acceso total al sistema'),
    ('01900000-0000-7000-8000-000000000002', 'Encargado',     'Operación completa salvo gestión de usuarios'),
    ('01900000-0000-7000-8000-000000000003', 'Cajero',        'Operación de caja y ventas'),
    ('01900000-0000-7000-8000-000000000004', 'Repositor',     'Recepción de mercadería y etiquetado');

INSERT INTO identidad.rol_permisos (rol_id, permiso) VALUES
    -- Administrador: todos
    ('01900000-0000-7000-8000-000000000001', 'vender'),
    ('01900000-0000-7000-8000-000000000001', 'anular_venta'),
    ('01900000-0000-7000-8000-000000000001', 'aplicar_descuento'),
    ('01900000-0000-7000-8000-000000000001', 'exceder_limite_credito'),
    ('01900000-0000-7000-8000-000000000001', 'confirmar_recepcion'),
    ('01900000-0000-7000-8000-000000000001', 'ajustar_stock'),
    ('01900000-0000-7000-8000-000000000001', 'modificar_precios'),
    ('01900000-0000-7000-8000-000000000001', 'gestionar_usuarios'),
    ('01900000-0000-7000-8000-000000000001', 'gestionar_clientes'),
    ('01900000-0000-7000-8000-000000000001', 'ver_reportes'),
    ('01900000-0000-7000-8000-000000000001', 'cerrar_caja'),
    ('01900000-0000-7000-8000-000000000001', 'abrir_caja'),
    ('01900000-0000-7000-8000-000000000001', 'gestionar_catalogo'),
    ('01900000-0000-7000-8000-000000000001', 'gestionar_proveedores'),
    -- Encargado: todo salvo gestionar_usuarios
    ('01900000-0000-7000-8000-000000000002', 'vender'),
    ('01900000-0000-7000-8000-000000000002', 'anular_venta'),
    ('01900000-0000-7000-8000-000000000002', 'aplicar_descuento'),
    ('01900000-0000-7000-8000-000000000002', 'exceder_limite_credito'),
    ('01900000-0000-7000-8000-000000000002', 'confirmar_recepcion'),
    ('01900000-0000-7000-8000-000000000002', 'ajustar_stock'),
    ('01900000-0000-7000-8000-000000000002', 'modificar_precios'),
    ('01900000-0000-7000-8000-000000000002', 'gestionar_clientes'),
    ('01900000-0000-7000-8000-000000000002', 'ver_reportes'),
    ('01900000-0000-7000-8000-000000000002', 'cerrar_caja'),
    ('01900000-0000-7000-8000-000000000002', 'abrir_caja'),
    ('01900000-0000-7000-8000-000000000002', 'gestionar_catalogo'),
    ('01900000-0000-7000-8000-000000000002', 'gestionar_proveedores'),
    -- Cajero
    ('01900000-0000-7000-8000-000000000003', 'vender'),
    ('01900000-0000-7000-8000-000000000003', 'aplicar_descuento'),
    ('01900000-0000-7000-8000-000000000003', 'abrir_caja'),
    ('01900000-0000-7000-8000-000000000003', 'cerrar_caja'),
    -- Repositor
    ('01900000-0000-7000-8000-000000000004', 'confirmar_recepcion'),
    ('01900000-0000-7000-8000-000000000004', 'ajustar_stock'),
    ('01900000-0000-7000-8000-000000000004', 'gestionar_catalogo');
