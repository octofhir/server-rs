-- Operational resources are independent of FHIR packages and available on cold boot.
CREATE TABLE _storage_providers (
 id text PRIMARY KEY,
 version bigint NOT NULL CHECK(version>0),
 resource jsonb NOT NULL CHECK(jsonb_typeof(resource)='object'),
 encrypted_credentials jsonb,
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 updated_by text NOT NULL
);
CREATE TABLE _storage_provider_history (
 provider_id text NOT NULL,
 version bigint NOT NULL,
 action text NOT NULL CHECK(action IN ('create','update','delete')),
 resource jsonb NOT NULL,
 actor text NOT NULL,
 recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(provider_id,version)
);
