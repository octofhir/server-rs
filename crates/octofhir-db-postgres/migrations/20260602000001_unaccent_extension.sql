-- Enable the `unaccent` extension for FHIR R4 accent-insensitive string search.
--
-- Per FHIR R4 §3.1.1.5.6 (https://hl7.org/fhir/R4/search.html#string),
-- string search defaults to "case-insensitive and accent-insensitive matching".
--
-- The unaccent() function strips combining diacritical marks (é → e, ñ → n,
-- ü → u, …). String search SQL wraps both indexed values and query values
-- with f_unaccent_lower(...) so the comparison is symmetric.
--
-- `f_unaccent_lower` is defined as IMMUTABLE so it can be used in expression
-- indexes (the standard `unaccent(text)` is STABLE because the dictionary
-- can change at runtime — we lock it to the default dictionary, which is
-- effectively immutable for our purposes).

CREATE EXTENSION IF NOT EXISTS unaccent;

CREATE OR REPLACE FUNCTION f_unaccent_lower(text)
RETURNS text AS $$
  SELECT lower(unaccent('public.unaccent', $1));
$$ LANGUAGE sql IMMUTABLE PARALLEL SAFE;
