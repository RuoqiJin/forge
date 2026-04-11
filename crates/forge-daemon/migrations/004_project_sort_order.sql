-- Add sort_order column for drag-and-drop project reordering in the UI.
ALTER TABLE projects ADD COLUMN sort_order INT NOT NULL DEFAULT 0;

-- Initialize sort_order from current alphabetical ordering.
WITH ranked AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY name) AS rn
    FROM projects
)
UPDATE projects SET sort_order = ranked.rn FROM ranked WHERE projects.id = ranked.id;
