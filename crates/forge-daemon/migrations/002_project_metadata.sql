-- Add language and status tracking to projects

DO $$ BEGIN
    ALTER TABLE projects ADD COLUMN language TEXT DEFAULT 'unknown';
EXCEPTION WHEN duplicate_column THEN NULL;
END $$;

DO $$ BEGIN
    ALTER TABLE projects ADD COLUMN status TEXT DEFAULT 'unmapped';
EXCEPTION WHEN duplicate_column THEN NULL;
END $$;

-- Backfill existing rows
UPDATE projects SET language = 'rust', status = 'mapped' WHERE language IS NULL OR language = 'unknown';
