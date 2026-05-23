-- Date search — tstzrange + GiST index.
--
-- Adds an `rng tstzrange` column to `search_idx_date` plus a GiST index that
-- covers every FHIR date prefix. The (param_code, rng) GiST index
-- requires the btree_gist extension to combine btree-style equality on
-- param_code with the GiST range key in a single index access.
--
-- See docs/architecture/date-search.md for the full design.

CREATE EXTENSION IF NOT EXISTS btree_gist;

-- `rng` materialises the half-open canonical range. Writers populate it from
-- `range_start` / `range_end`; keeping it as a plain column avoids generated
-- column syntax differences in the migration runner / test container.
--
-- This is a dev database with no compatibility requirement. Recreate the
-- column explicitly so failed/partial prior attempts cannot leave a bad shape.
ALTER TABLE search_idx_date
    DROP COLUMN IF EXISTS rng;

ALTER TABLE search_idx_date
    ADD COLUMN rng TSTZRANGE NOT NULL;

-- GiST index on (param_code, rng). One Bitmap Index Scan satisfies every
-- prefix (eq / ne / gt / ge / lt / le / sa / eb / ap) — see architecture doc §4.
CREATE INDEX IF NOT EXISTS search_idx_date_rng_gist
    ON search_idx_date USING gist (param_code, rng);

COMMENT ON COLUMN search_idx_date.rng IS
    'Half-open tstzrange [range_start, range_end). Drives every FHIR date prefix via one range operator. See docs/architecture/date-search.md.';
