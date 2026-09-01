-- Phase 0a establishes only what the health check and later phases need to exist.
-- Schema proper arrives in Phase 1; see docs/DATA.md.

-- Trigram search, used by lapidary-index from Phase 2. Ships with the postgres image.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
