-- Forge initial schema

CREATE TABLE IF NOT EXISTS projects (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    root_path   TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ast_nodes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path       TEXT NOT NULL,
    symbol_name     TEXT NOT NULL,
    symbol_kind     TEXT NOT NULL,   -- function, struct, enum, trait, impl, mod
    start_line      INT NOT NULL,
    end_line        INT NOT NULL,
    signature       TEXT,
    docstring       TEXT,
    is_exported     BOOLEAN NOT NULL DEFAULT false,
    parent_symbol   TEXT,            -- enclosing symbol (e.g. impl block)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_ast_nodes_project ON ast_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_ast_nodes_file ON ast_nodes(project_id, file_path);
CREATE INDEX IF NOT EXISTS idx_ast_nodes_symbol ON ast_nodes(project_id, symbol_name);
