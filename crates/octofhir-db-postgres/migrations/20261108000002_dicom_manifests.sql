-- Trusted, metadata-only connector attestation; never writable through FHIR CRUD.
CREATE TABLE dicom_manifests (
 connection_name text NOT NULL,
 study_uid text NOT NULL,
 resource_id text NOT NULL,
 patient_id text NOT NULL,
 source_hash text NOT NULL,
 manifest_hash text NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(connection_name,study_uid),
 UNIQUE(resource_id)
);
