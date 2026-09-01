-- Verified at first boot rather than discovered in Phase 6.
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Turkish is the planned second locale; its snowball config ships with PostgreSQL.
-- Fail loudly at init if that ever stops being true.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_ts_config WHERE cfgname = 'turkish') THEN
    RAISE EXCEPTION 'The turkish text search configuration is missing from this PostgreSQL build.';
  END IF;
END
$$;
