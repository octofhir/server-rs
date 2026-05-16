//! PostgreSQL implementation of the PackageStore and SearchStorage traits
//! from octofhir-canonical-manager.
//!
//! This module provides a PostgreSQL backend for storing FHIR packages and resources
//! from Implementation Guides, enabling efficient querying and resolution of
//! canonical URLs in a server environment.
//!
//! Also provides storage for pre-converted FHIRSchemas to enable on-demand loading
//! and reduce memory usage by using an LRU cache backed by the database.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::row::Row;
use sqlx_postgres::PgPool;
use tracing::{debug, info, instrument, warn};

use octofhir_canonical_manager::domain::{PackageInfo, ResourceIndex};
use octofhir_canonical_manager::error::{FcmError, StorageError};
use octofhir_canonical_manager::package::{ExtractedPackage, FhirResource};
use octofhir_canonical_manager::traits::{PackageStore, SearchStorage};

use crate::SchemaManager;

/// PostgreSQL storage backend for FHIR packages from canonical manager.
///
/// Implements both `PackageStore` and `SearchStorage` traits, enabling
/// the FHIR server to use PostgreSQL for storing and querying FHIR
/// conformance resources from Implementation Guides.
#[derive(Debug, Clone)]
pub struct PostgresPackageStore {
    pool: PgPool,
}

/// Helper struct for StructureDefinition fields
struct SdFields {
    kind: Option<String>,
    derivation: Option<String>,
    sd_type: Option<String>,
    base_definition: Option<String>,
    is_abstract: Option<bool>,
    impose_profiles: Option<Vec<String>>,
    characteristics: Option<Vec<String>>,
    flavor: Option<String>,
}

/// Stored FHIRSchema record from the database.
///
/// Represents a pre-converted FHIRSchema that can be loaded on-demand
/// to reduce memory usage compared to loading all schemas at startup.
#[derive(Debug, Clone)]
pub struct FhirSchemaRecord {
    /// Canonical URL of the source StructureDefinition
    pub url: String,
    /// StructureDefinition version
    pub version: Option<String>,
    /// Source package name
    pub package_name: String,
    /// Source package version
    pub package_version: String,
    /// FHIR version (R4, R4B, R5, R6)
    pub fhir_version: String,
    /// Schema type: 'resource', 'complex-type', 'extension', 'primitive-type', 'logical'
    pub schema_type: String,
    /// The FHIRSchema JSON content
    pub content: Value,
    /// Hash for cache invalidation
    pub content_hash: String,
}

/// Summary information for a FHIRSchema.
///
/// Used for resource type listings with package info.
#[derive(Debug, Clone)]
pub struct FhirSchemaInfo {
    /// Name of the resource type
    pub name: String,
    /// Canonical URL of the resource
    pub url: Option<String>,
    /// Source package name
    pub package_name: String,
    /// Source package version
    pub package_version: String,
}

/// Helper function to convert sqlx error to FcmError
fn db_error(e: sqlx_core::error::Error) -> FcmError {
    FcmError::Storage(StorageError::DatabaseError {
        message: e.to_string(),
    })
}

/// Convert a database row to ResourceIndex
fn row_to_resource_index(row: &sqlx_postgres::PgRow) -> ResourceIndex {
    let url: Option<String> = row.get("url");
    let impose_profiles: Option<Value> = row.get("sd_impose_profiles");
    let characteristics: Option<Value> = row.get("sd_characteristics");

    ResourceIndex {
        canonical_url: url.unwrap_or_default(),
        resource_type: row.get("resource_type"),
        package_name: row.get("package_name"),
        package_version: row.get("package_version"),
        fhir_version: row.get("fhir_version"),
        file_path: PathBuf::new(),
        id: row.get("resource_id"),
        name: row.get("name"),
        version: row.get("version"),
        sd_kind: row.get("sd_kind"),
        sd_derivation: row.get("sd_derivation"),
        sd_type: row.get("sd_type"),
        sd_base_definition: row.get("sd_base_definition"),
        sd_abstract: row.get("sd_abstract"),
        sd_impose_profiles: impose_profiles.and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        }),
        sd_characteristics: characteristics.and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        }),
        sd_flavor: row.get("sd_flavor"),
    }
}

impl PostgresPackageStore {
    /// Creates a new PostgreSQL package store.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns a reference to the connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Extract StructureDefinition-specific fields from content
    fn extract_sd_fields(content: &Value) -> SdFields {
        SdFields {
            kind: content
                .get("kind")
                .and_then(|v| v.as_str())
                .map(String::from),
            derivation: content
                .get("derivation")
                .and_then(|v| v.as_str())
                .map(String::from),
            sd_type: content
                .get("type")
                .and_then(|v| v.as_str())
                .map(String::from),
            base_definition: content
                .get("baseDefinition")
                .and_then(|v| v.as_str())
                .map(String::from),
            is_abstract: content.get("abstract").and_then(|v| v.as_bool()),
            impose_profiles: content.get("imposeProfile").and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
            }),
            characteristics: content.get("characteristics").and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
            }),
            flavor: Self::determine_sd_flavor(content),
        }
    }

    /// Determine the SD flavor based on kind, derivation, and type
    fn determine_sd_flavor(content: &Value) -> Option<String> {
        let kind = content.get("kind").and_then(|v| v.as_str());
        let derivation = content.get("derivation").and_then(|v| v.as_str());
        let sd_type = content.get("type").and_then(|v| v.as_str());

        match (kind, derivation) {
            (Some("resource"), Some("specialization")) => Some("resource".to_string()),
            (Some("resource"), Some("constraint")) => Some("profile".to_string()),
            (Some("complex-type"), Some("specialization")) => Some("complex-type".to_string()),
            (Some("complex-type"), Some("constraint")) => {
                if sd_type == Some("Extension") {
                    Some("extension".to_string())
                } else {
                    Some("profile".to_string())
                }
            }
            (Some("primitive-type"), _) => Some("primitive-type".to_string()),
            (Some("logical"), _) => Some("logical".to_string()),
            _ => None,
        }
    }

    /// Compute manifest hash from package contents
    fn compute_manifest_hash(package: &ExtractedPackage) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        package.name.hash(&mut hasher);
        package.version.hash(&mut hasher);
        package.resources.len().hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Load package from in-memory resources (for embedded internal package).
    ///
    /// This method is used for single-binary deployment where internal IGs are
    /// embedded in the binary. It loads resources directly into the FCM schema
    /// without requiring filesystem access.
    ///
    /// # Arguments
    ///
    /// * `name` - Package name (e.g., "octofhir.internal")
    /// * `version` - Package version (e.g., "0.1.0")
    /// * `fhir_version` - FHIR version from config (never hardcoded!)
    /// * `resources` - Vector of (resource_type, content) tuples
    #[instrument(skip(self, resources), fields(name = %name, version = %version, count = %resources.len()))]
    pub async fn load_from_embedded(
        &self,
        name: &str,
        version: &str,
        fhir_version: &str,
        resources: Vec<(String, Value)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "Loading embedded package {}@{} (FHIR {}) with {} resources",
            name,
            version,
            fhir_version,
            resources.len()
        );

        // Compute a content-derived manifest hash so warm-boots can short-circuit.
        // We hash (resource_type, content) tuples in a stable order so identical
        // embedded bundles produce the same hash regardless of vec ordering.
        let manifest_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut entries: Vec<(String, String)> = resources
                .iter()
                .map(|(rt, c)| (rt.clone(), serde_json::to_string(c).unwrap_or_default()))
                .collect();
            entries.sort();
            let mut hasher = DefaultHasher::new();
            name.hash(&mut hasher);
            version.hash(&mut hasher);
            fhir_version.hash(&mut hasher);
            for (rt, body) in &entries {
                rt.hash(&mut hasher);
                body.hash(&mut hasher);
            }
            format!("embedded:{:016x}", hasher.finish())
        };

        // Fast path: if a row already exists with the same manifest hash AND fhir_version,
        // nothing changed since the last install — skip the re-DELETE + re-INSERT churn.
        // This is the dominant cost on warm-boot.
        let existing: Option<(String, String)> = sqlx_core::query_as::query_as::<_, (String, String)>(
            "SELECT manifest_hash, fhir_version FROM fcm.packages WHERE name = $1 AND version = $2",
        )
        .bind(name)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((existing_hash, existing_fhir)) = existing {
            if existing_hash == manifest_hash && existing_fhir == fhir_version {
                info!(
                    "Embedded package {}@{} already at hash {} — skipping reload",
                    name, version, manifest_hash
                );
                return Ok(());
            }
        }

        // Insert package metadata with priority 100 (higher than external packages)
        query(
            r#"
            INSERT INTO fcm.packages (name, version, fhir_version, manifest_hash, resource_count, priority)
            VALUES ($1, $2, $3, $4, $5, 100)
            ON CONFLICT (name, version) DO UPDATE SET
                fhir_version = EXCLUDED.fhir_version,
                manifest_hash = EXCLUDED.manifest_hash,
                resource_count = EXCLUDED.resource_count,
                priority = EXCLUDED.priority,
                installed_at = NOW()
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(fhir_version)
        .bind(&manifest_hash)
        .bind(resources.len() as i32)
        .execute(&self.pool)
        .await?;

        // Delete existing resources for this package (for re-installation)
        query("DELETE FROM fcm.resources WHERE package_name = $1 AND package_version = $2")
            .bind(name)
            .bind(version)
            .execute(&self.pool)
            .await?;

        // Delete existing FHIRSchemas for this package (they will be regenerated on-demand)
        let deleted_schemas =
            query("DELETE FROM fcm.fhirschemas WHERE package_name = $1 AND package_version = $2")
                .bind(name)
                .bind(version)
                .execute(&self.pool)
                .await?
                .rows_affected();

        if deleted_schemas > 0 {
            info!(
                "Deleted {} old FHIRSchemas for package {}@{} (will regenerate on-demand)",
                deleted_schemas, name, version
            );
        }

        // Batch insert all resources via UNNEST (one round-trip).
        let len = resources.len();
        let mut resource_types: Vec<String> = Vec::with_capacity(len);
        let mut resource_ids: Vec<Option<String>> = Vec::with_capacity(len);
        let mut urls: Vec<Option<String>> = Vec::with_capacity(len);
        let mut names: Vec<Option<String>> = Vec::with_capacity(len);
        let mut versions: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sd_kinds: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sd_derivations: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sd_types: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sd_base_defs: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sd_abstracts: Vec<Option<bool>> = Vec::with_capacity(len);
        let mut sd_impose: Vec<Option<Value>> = Vec::with_capacity(len);
        let mut sd_chars: Vec<Option<Value>> = Vec::with_capacity(len);
        let mut sd_flavors: Vec<Option<String>> = Vec::with_capacity(len);
        let mut content_hashes: Vec<String> = Vec::with_capacity(len);
        let mut contents: Vec<Value> = Vec::with_capacity(len);

        for (resource_type, content) in &resources {
            let sd_fields = Self::extract_sd_fields(content);
            resource_types.push(resource_type.clone());
            resource_ids.push(content.get("id").and_then(|v| v.as_str()).map(String::from));
            urls.push(content.get("url").and_then(|v| v.as_str()).map(String::from));
            names.push(
                content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            );
            versions.push(
                content
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            );
            sd_kinds.push(sd_fields.kind);
            sd_derivations.push(sd_fields.derivation);
            sd_types.push(sd_fields.sd_type);
            sd_base_defs.push(sd_fields.base_definition);
            sd_abstracts.push(sd_fields.is_abstract);
            sd_impose.push(
                sd_fields
                    .impose_profiles
                    .and_then(|v| serde_json::to_value(v).ok()),
            );
            sd_chars.push(
                sd_fields
                    .characteristics
                    .and_then(|v| serde_json::to_value(v).ok()),
            );
            sd_flavors.push(sd_fields.flavor);

            let content_str = serde_json::to_string(content)?;
            let hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                content_str.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            };
            content_hashes.push(hash);
            contents.push(content.clone());
        }

        query(
            r#"
            INSERT INTO fcm.resources (
                resource_type, resource_id, url, name, version,
                sd_kind, sd_derivation, sd_type, sd_base_definition, sd_abstract,
                sd_impose_profiles, sd_characteristics, sd_flavor,
                package_name, package_version, fhir_version, content_hash, content
            )
            SELECT
                t.rt, t.rid, t.url, t.nm, t.ver,
                t.kind, t.deriv, t.styp, t.bdef, t.abs_,
                t.imp, t.chr, t.flav,
                $14, $15, $16, t.hsh, t.cnt
            FROM UNNEST(
                $1::text[], $2::text[], $3::text[], $4::text[], $5::text[],
                $6::text[], $7::text[], $8::text[], $9::text[], $10::bool[],
                $11::jsonb[], $12::jsonb[], $13::text[],
                $17::text[], $18::jsonb[]
            ) AS t(rt, rid, url, nm, ver, kind, deriv, styp, bdef, abs_, imp, chr, flav, hsh, cnt)
            "#,
        )
        .bind(&resource_types)
        .bind(&resource_ids)
        .bind(&urls)
        .bind(&names)
        .bind(&versions)
        .bind(&sd_kinds)
        .bind(&sd_derivations)
        .bind(&sd_types)
        .bind(&sd_base_defs)
        .bind(&sd_abstracts)
        .bind(&sd_impose)
        .bind(&sd_chars)
        .bind(&sd_flavors)
        .bind(name)
        .bind(version)
        .bind(fhir_version)
        .bind(&content_hashes)
        .bind(&contents)
        .execute(&self.pool)
        .await?;

        info!(
            "Successfully loaded embedded package {}@{} with {} resources to FCM",
            name,
            version,
            resources.len()
        );

        Ok(())
    }

    /// Common SQL for selecting resource fields
    const RESOURCE_SELECT: &'static str = r#"
        SELECT
            resource_type, resource_id, url, name, version,
            package_name, package_version, fhir_version, content_hash,
            sd_kind, sd_derivation, sd_type, sd_base_definition, sd_abstract,
            sd_impose_profiles, sd_characteristics, sd_flavor
        FROM fcm.resources
    "#;

    /// Creates database tables for all resource-kind StructureDefinitions in FCM.
    ///
    /// This method queries the FCM resources table for StructureDefinitions with
    /// `sd_kind = 'resource'` or `sd_kind = 'logical'` and ensures that corresponding
    /// tables exist in the public schema.
    ///
    /// # Returns
    ///
    /// Returns the number of tables created (excluding already existing tables).
    #[instrument(skip(self))]
    pub async fn ensure_resource_tables(&self) -> Result<usize, FcmError> {
        info!("Ensuring database tables for all FHIR resource types from FCM");

        // Query FCM for all resource-kind and logical-kind StructureDefinitions
        // Use 'name' instead of 'sd_type' because for logical models (like ViewDefinition),
        // sd_type contains the canonical URL, not the simple type name.
        // Exclude profiles (sd_derivation = 'constraint') as they use the same table as their base.
        let resource_types: Vec<String> = query_scalar(
            r#"
            SELECT DISTINCT name
            FROM fcm.resources
            WHERE resource_type = 'StructureDefinition'
              AND (sd_kind = 'resource' OR sd_kind = 'logical')
              AND (sd_derivation IS NULL OR sd_derivation = 'specialization')
              AND name IS NOT NULL
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        info!(
            "Found {} resource types to create tables for",
            resource_types.len()
        );

        // Define the shared archive_to_history() function once before fanning
        // out — every history trigger uses it, but defining it from many
        // concurrent connections would serialize on the catalog lock.
        SchemaManager::ensure_archive_function(&self.pool)
            .await
            .map_err(|e| {
                FcmError::Storage(StorageError::DatabaseError {
                    message: format!("ensure_archive_function failed: {e}"),
                })
            })?;

        // Warm-DB fast path: query pg_class once to find which main tables and
        // partitions already exist. Skip per-resource raw_sql for types whose
        // entire schema is already in place. Partition creates on
        // `search_idx_reference` / `search_idx_date` take AccessExclusiveLock
        // on the parent, so even "CREATE TABLE IF NOT EXISTS" partitions
        // serialize across the pool — avoiding the call is the only real win.
        let existing_main: std::collections::HashSet<String> = match sqlx_core::query_as::query_as::<
            _,
            (String,),
        >(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows.into_iter().map(|(t,)| t).collect(),
            Err(e) => {
                warn!(error = %e, "Failed to probe existing tables; falling back to unconditional create");
                std::collections::HashSet::new()
            }
        };

        let need_create: Vec<String> = resource_types
            .into_iter()
            .filter(|rt| {
                let table = rt.to_lowercase();
                let main_present = existing_main.contains(&table);
                let ref_part_present = existing_main.contains(&format!("search_idx_reference_{table}"));
                let date_part_present = existing_main.contains(&format!("search_idx_date_{table}"));
                let is_internal = matches!(
                    table.as_str(),
                    "user"
                        | "client"
                        | "session"
                        | "authsession"
                        | "accesspolicy"
                        | "refreshtoken"
                        | "revokedtoken"
                        | "identityprovider"
                        | "role"
                        | "app"
                        | "customoperation"
                        | "appsubscription"
                        | "notificationlog"
                        | "notificationprovider"
                        | "notificationtemplate"
                );
                // Internal resources don't get partitions — skip if main exists.
                if is_internal {
                    return !main_present;
                }
                !(main_present && ref_part_present && date_part_present)
            })
            .collect();

        info!(
            already_present = (existing_main.iter().filter(|t| !t.starts_with("search_idx_")).count()),
            need_create = need_create.len(),
            "Resource-schema cold/warm decision"
        );

        if need_create.is_empty() {
            return Ok(0);
        }

        // Parallelize schema creation. Each call now issues one round-trip
        // (multi-statement DDL via raw_sql). Concurrency is bounded so we
        // don't exhaust the pool — leave headroom for the rest of bootstrap.
        let concurrency = std::cmp::min(need_create.len(), 16);
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let mut set = tokio::task::JoinSet::new();

        for resource_type in need_create {
            let pool = self.pool.clone();
            let sem = semaphore.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.ok();
                let schema_manager = SchemaManager::new(pool);
                let result = schema_manager.create_resource_schema(&resource_type).await;
                (resource_type, result)
            });
        }

        let mut created_count = 0;
        while let Some(join_result) = set.join_next().await {
            match join_result {
                Ok((rt, Ok(()))) => {
                    debug!("Created schema for resource type: {}", rt);
                    created_count += 1;
                }
                Ok((rt, Err(e))) => {
                    warn!(resource_type = %rt, error = %e, "Failed to create table for resource type");
                }
                Err(e) => {
                    warn!(error = %e, "Schema-creation task panicked");
                }
            }
        }

        info!(
            "Created/verified {} tables for FHIR resource types",
            created_count
        );
        Ok(created_count)
    }

    // ==================== FHIRSchema Storage Methods ====================

    /// Store a FHIRSchema in the database.
    ///
    /// This method stores a pre-converted FHIRSchema for on-demand loading.
    /// Uses upsert semantics - if a schema with the same URL and package exists,
    /// it will be updated if the content hash differs.
    ///
    /// # Arguments
    ///
    /// * `url` - Canonical URL of the source StructureDefinition
    /// * `version` - StructureDefinition version
    /// * `package_name` - Source package name
    /// * `package_version` - Source package version
    /// * `fhir_version` - FHIR version (e.g., "4.0.1", "4.3.0", "5.0.0")
    /// * `schema_type` - Type of schema: "resource", "complex-type", "extension", etc.
    /// * `content` - The FHIRSchema JSON content
    #[instrument(skip(self, content), fields(url = %url, package = %format!("{}@{}", package_name, package_version)))]
    pub async fn store_fhirschema(
        &self,
        url: &str,
        version: Option<&str>,
        package_name: &str,
        package_version: &str,
        fhir_version: &str,
        schema_type: &str,
        content: &Value,
    ) -> Result<(), FcmError> {
        // Compute content hash for cache invalidation
        let content_str = serde_json::to_string(content).unwrap_or_default();
        let content_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            content_str.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        };

        debug!(
            "Storing FHIRSchema for {} (type: {}, hash: {})",
            url, schema_type, content_hash
        );

        query(
            r#"
            INSERT INTO fcm.fhirschemas (
                url, version, package_name, package_version, fhir_version,
                schema_type, content, content_hash
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (url, package_name, package_version) DO UPDATE SET
                version = EXCLUDED.version,
                fhir_version = EXCLUDED.fhir_version,
                schema_type = EXCLUDED.schema_type,
                content = EXCLUDED.content,
                content_hash = EXCLUDED.content_hash,
                created_at = NOW()
            WHERE fcm.fhirschemas.content_hash != EXCLUDED.content_hash
            "#,
        )
        .bind(url)
        .bind(version)
        .bind(package_name)
        .bind(package_version)
        .bind(fhir_version)
        .bind(schema_type)
        .bind(content)
        .bind(&content_hash)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Store multiple FHIRSchemas in a single UNNEST INSERT (one round-trip).
    #[instrument(skip(self, schemas), fields(count = schemas.len()))]
    pub async fn store_fhirschemas_batch(
        &self,
        schemas: &[(
            &str,         // url
            Option<&str>, // version
            &str,         // package_name
            &str,         // package_version
            &str,         // fhir_version
            &str,         // schema_type
            &Value,       // content
        )],
    ) -> Result<usize, FcmError> {
        if schemas.is_empty() {
            return Ok(0);
        }

        info!("Storing {} FHIRSchemas in batch (UNNEST)", schemas.len());

        let len = schemas.len();
        let mut urls: Vec<&str> = Vec::with_capacity(len);
        let mut versions: Vec<Option<&str>> = Vec::with_capacity(len);
        let mut package_names: Vec<&str> = Vec::with_capacity(len);
        let mut package_versions: Vec<&str> = Vec::with_capacity(len);
        let mut fhir_versions: Vec<&str> = Vec::with_capacity(len);
        let mut schema_types: Vec<&str> = Vec::with_capacity(len);
        let mut contents: Vec<Value> = Vec::with_capacity(len);
        let mut hashes: Vec<String> = Vec::with_capacity(len);

        for (url, version, pkg_name, pkg_version, fhir_ver, schema_type, content) in schemas {
            urls.push(url);
            versions.push(*version);
            package_names.push(pkg_name);
            package_versions.push(pkg_version);
            fhir_versions.push(fhir_ver);
            schema_types.push(schema_type);
            let content_str = serde_json::to_string(content).unwrap_or_default();
            let hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                content_str.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            };
            hashes.push(hash);
            contents.push((*content).clone());
        }

        let owned_versions: Vec<Option<String>> =
            versions.iter().map(|v| v.map(|s| s.to_string())).collect();
        let owned_urls: Vec<String> = urls.iter().map(|s| s.to_string()).collect();
        let owned_pkg_names: Vec<String> = package_names.iter().map(|s| s.to_string()).collect();
        let owned_pkg_versions: Vec<String> = package_versions.iter().map(|s| s.to_string()).collect();
        let owned_fhir_versions: Vec<String> = fhir_versions.iter().map(|s| s.to_string()).collect();
        let owned_schema_types: Vec<String> = schema_types.iter().map(|s| s.to_string()).collect();

        query(
            r#"
            INSERT INTO fcm.fhirschemas (
                url, version, package_name, package_version, fhir_version,
                schema_type, content, content_hash
            )
            SELECT t.url, t.version, t.pkg, t.pkg_ver, t.fhir, t.stype, t.content, t.hash
            FROM UNNEST(
                $1::text[], $2::text[], $3::text[], $4::text[],
                $5::text[], $6::text[], $7::jsonb[], $8::text[]
            ) AS t(url, version, pkg, pkg_ver, fhir, stype, content, hash)
            ON CONFLICT (url, package_name, package_version) DO UPDATE SET
                version = EXCLUDED.version,
                fhir_version = EXCLUDED.fhir_version,
                schema_type = EXCLUDED.schema_type,
                content = EXCLUDED.content,
                content_hash = EXCLUDED.content_hash,
                created_at = NOW()
            WHERE fcm.fhirschemas.content_hash != EXCLUDED.content_hash
            "#,
        )
        .bind(&owned_urls)
        .bind(&owned_versions)
        .bind(&owned_pkg_names)
        .bind(&owned_pkg_versions)
        .bind(&owned_fhir_versions)
        .bind(&owned_schema_types)
        .bind(&contents)
        .bind(&hashes)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        info!("Successfully stored {} FHIRSchemas", len);
        Ok(len)
    }

    /// Get a FHIRSchema by canonical URL.
    ///
    /// Returns the first matching schema, preferring higher-priority packages.
    /// If multiple packages contain the same URL, the one with highest priority is returned.
    #[instrument(skip(self))]
    pub async fn get_fhirschema(&self, url: &str) -> Result<Option<FhirSchemaRecord>, FcmError> {
        debug!("Getting FHIRSchema for URL: {}", url);

        let row = query(
            r#"
            SELECT
                s.url, s.version, s.package_name, s.package_version,
                s.fhir_version, s.schema_type, s.content, s.content_hash
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.url = $1
            ORDER BY p.priority DESC, s.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(url)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(row.map(|r| FhirSchemaRecord {
            url: r.get("url"),
            version: r.get("version"),
            package_name: r.get("package_name"),
            package_version: r.get("package_version"),
            fhir_version: r.get("fhir_version"),
            schema_type: r.get("schema_type"),
            content: r.get("content"),
            content_hash: r.get("content_hash"),
        }))
    }

    /// Get a FHIRSchema by canonical URL and FHIR version.
    ///
    /// Useful when the server needs a schema for a specific FHIR version.
    #[instrument(skip(self))]
    pub async fn get_fhirschema_for_fhir_version(
        &self,
        url: &str,
        fhir_version: &str,
    ) -> Result<Option<FhirSchemaRecord>, FcmError> {
        debug!(
            "Getting FHIRSchema for URL: {} (FHIR {})",
            url, fhir_version
        );

        let row = query(
            r#"
            SELECT
                s.url, s.version, s.package_name, s.package_version,
                s.fhir_version, s.schema_type, s.content, s.content_hash
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.url = $1 AND s.fhir_version = $2
            ORDER BY p.priority DESC, s.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(url)
        .bind(fhir_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(row.map(|r| FhirSchemaRecord {
            url: r.get("url"),
            version: r.get("version"),
            package_name: r.get("package_name"),
            package_version: r.get("package_version"),
            fhir_version: r.get("fhir_version"),
            schema_type: r.get("schema_type"),
            content: r.get("content"),
            content_hash: r.get("content_hash"),
        }))
    }

    /// Get a FHIRSchema by canonical URL from a specific package.
    #[instrument(skip(self))]
    pub async fn get_fhirschema_from_package(
        &self,
        url: &str,
        package_name: &str,
        package_version: &str,
    ) -> Result<Option<FhirSchemaRecord>, FcmError> {
        debug!(
            "Getting FHIRSchema for URL: {} from {}@{}",
            url, package_name, package_version
        );

        let row = query(
            r#"
            SELECT
                url, version, package_name, package_version,
                fhir_version, schema_type, content, content_hash
            FROM fcm.fhirschemas
            WHERE url = $1 AND package_name = $2 AND package_version = $3
            "#,
        )
        .bind(url)
        .bind(package_name)
        .bind(package_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(row.map(|r| FhirSchemaRecord {
            url: r.get("url"),
            version: r.get("version"),
            package_name: r.get("package_name"),
            package_version: r.get("package_version"),
            fhir_version: r.get("fhir_version"),
            schema_type: r.get("schema_type"),
            content: r.get("content"),
            content_hash: r.get("content_hash"),
        }))
    }

    /// List all FHIRSchemas for a specific package.
    #[instrument(skip(self))]
    pub async fn list_fhirschemas_for_package(
        &self,
        package_name: &str,
        package_version: &str,
    ) -> Result<Vec<FhirSchemaRecord>, FcmError> {
        debug!(
            "Listing FHIRSchemas for package {}@{}",
            package_name, package_version
        );

        let rows = query(
            r#"
            SELECT
                url, version, package_name, package_version,
                fhir_version, schema_type, content, content_hash
            FROM fcm.fhirschemas
            WHERE package_name = $1 AND package_version = $2
            ORDER BY schema_type, url
            "#,
        )
        .bind(package_name)
        .bind(package_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows
            .iter()
            .map(|r| FhirSchemaRecord {
                url: r.get("url"),
                version: r.get("version"),
                package_name: r.get("package_name"),
                package_version: r.get("package_version"),
                fhir_version: r.get("fhir_version"),
                schema_type: r.get("schema_type"),
                content: r.get("content"),
                content_hash: r.get("content_hash"),
            })
            .collect())
    }

    /// List all FHIRSchemas of a specific type (resource, complex-type, etc.).
    #[instrument(skip(self))]
    pub async fn list_fhirschemas_by_type(
        &self,
        schema_type: &str,
    ) -> Result<Vec<FhirSchemaRecord>, FcmError> {
        debug!("Listing FHIRSchemas of type: {}", schema_type);

        let rows = query(
            r#"
            SELECT
                s.url, s.version, s.package_name, s.package_version,
                s.fhir_version, s.schema_type, s.content, s.content_hash
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.schema_type = $1
            ORDER BY p.priority DESC, s.url
            "#,
        )
        .bind(schema_type)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows
            .iter()
            .map(|r| FhirSchemaRecord {
                url: r.get("url"),
                version: r.get("version"),
                package_name: r.get("package_name"),
                package_version: r.get("package_version"),
                fhir_version: r.get("fhir_version"),
                schema_type: r.get("schema_type"),
                content: r.get("content"),
                content_hash: r.get("content_hash"),
            })
            .collect())
    }

    /// Get the count of FHIRSchemas in the database.
    #[instrument(skip(self))]
    pub async fn count_fhirschemas(&self) -> Result<i64, FcmError> {
        let count: i64 = query_scalar("SELECT COUNT(*) FROM fcm.fhirschemas")
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(count)
    }

    /// Count FHIRSchemas already stored for a specific (package, version).
    ///
    /// Lets cold-boot fast-path detect that a package's schemas are already
    /// converted on warm DB, so the converter can skip the heavy
    /// translate + serialize + UNNEST INSERT pipeline.
    pub async fn count_fhirschemas_for_package(
        &self,
        package_name: &str,
        package_version: &str,
    ) -> Result<i64, FcmError> {
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM fcm.fhirschemas WHERE package_name = $1 AND package_version = $2",
        )
        .bind(package_name)
        .bind(package_version)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(count)
    }

    /// Delete all FHIRSchemas for a specific package.
    ///
    /// This is called automatically via CASCADE when the package is deleted,
    /// but can be called explicitly for partial updates.
    #[instrument(skip(self))]
    pub async fn delete_fhirschemas_for_package(
        &self,
        package_name: &str,
        package_version: &str,
    ) -> Result<u64, FcmError> {
        info!(
            "Deleting FHIRSchemas for package {}@{}",
            package_name, package_version
        );

        let result =
            query("DELETE FROM fcm.fhirschemas WHERE package_name = $1 AND package_version = $2")
                .bind(package_name)
                .bind(package_version)
                .execute(&self.pool)
                .await
                .map_err(db_error)?;

        let deleted = result.rows_affected();
        info!(
            "Deleted {} FHIRSchemas for package {}@{}",
            deleted, package_name, package_version
        );

        Ok(deleted)
    }

    /// Check if a FHIRSchema exists by URL (useful for cache warming decisions).
    #[instrument(skip(self))]
    pub async fn fhirschema_exists(&self, url: &str) -> Result<bool, FcmError> {
        let exists: bool =
            query_scalar("SELECT EXISTS(SELECT 1 FROM fcm.fhirschemas WHERE url = $1)")
                .bind(url)
                .fetch_one(&self.pool)
                .await
                .map_err(db_error)?;

        Ok(exists)
    }

    /// Get a FHIRSchema by type name (e.g., "Patient", "Observation").
    ///
    /// This looks up the schema by the `name` field in the FHIRSchema content.
    /// Used by the model provider for on-demand schema loading.
    #[instrument(skip(self))]
    pub async fn get_fhirschema_by_name(
        &self,
        name: &str,
        fhir_version: &str,
    ) -> Result<Option<FhirSchemaRecord>, FcmError> {
        debug!(
            "Getting FHIRSchema by name: {} (FHIR {})",
            name, fhir_version
        );

        let row = query(
            r#"
            SELECT
                s.url, s.version, s.package_name, s.package_version,
                s.fhir_version, s.schema_type, s.content, s.content_hash
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.content->>'name' = $1 AND s.fhir_version = $2
            ORDER BY p.priority DESC, s.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(fhir_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(row.map(|r| FhirSchemaRecord {
            url: r.get("url"),
            version: r.get("version"),
            package_name: r.get("package_name"),
            package_version: r.get("package_version"),
            fhir_version: r.get("fhir_version"),
            schema_type: r.get("schema_type"),
            content: r.get("content"),
            content_hash: r.get("content_hash"),
        }))
    }

    /// Get a FHIRSchema by canonical URL (for meta.profile validation).
    ///
    /// This is an alias for get_fhirschema_for_fhir_version that makes
    /// the use case clearer - validating resources against profiles
    /// specified in meta.profile.
    #[instrument(skip(self))]
    pub async fn get_fhirschema_by_url(
        &self,
        url: &str,
        fhir_version: &str,
    ) -> Result<Option<FhirSchemaRecord>, FcmError> {
        self.get_fhirschema_for_fhir_version(url, fhir_version)
            .await
    }

    /// List all FHIRSchema names of a specific type (resource, complex-type, etc.).
    ///
    /// Returns just the names (from content->'name'), not full records.
    /// Used by ModelProvider for get_resource_types(), get_complex_types(), etc.
    #[instrument(skip(self))]
    pub async fn list_fhirschema_names_by_type(
        &self,
        schema_type: &str,
        fhir_version: &str,
    ) -> Result<Vec<String>, FcmError> {
        debug!(
            "Listing FHIRSchema names of type: {} (FHIR {})",
            schema_type, fhir_version
        );

        // Use DISTINCT to avoid duplicates when same schema exists in multiple packages
        // Order by priority to get highest priority first, then take distinct names
        let names: Vec<String> = query_scalar(
            r#"
            SELECT DISTINCT ON (s.content->>'name') s.content->>'name' as name
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.schema_type = $1 AND s.fhir_version = $2
            ORDER BY s.content->>'name', p.priority DESC
            "#,
        )
        .bind(schema_type)
        .bind(fhir_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(names)
    }

    /// List FHIRSchema names for multiple types, excluding profiles.
    ///
    /// Returns just the names (from content->'name'), not full records.
    /// Filters out profiles (derivation = 'constraint') to only return base definitions.
    /// Used by ModelProvider for UI resource type lists.
    #[instrument(skip(self))]
    pub async fn list_fhirschema_names_by_kinds_excluding_profiles(
        &self,
        schema_types: &[&str],
        fhir_version: &str,
    ) -> Result<Vec<String>, FcmError> {
        debug!(
            "Listing FHIRSchema names of types: {:?} (FHIR {}), excluding profiles",
            schema_types, fhir_version
        );

        // Use DISTINCT to avoid duplicates when same schema exists in multiple packages
        // Order by priority to get highest priority first, then take distinct names
        // Filter out profiles (derivation = 'constraint')
        let names: Vec<String> = query_scalar(
            r#"
            SELECT DISTINCT ON (s.content->>'name') s.content->>'name' as name
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.schema_type = ANY($1)
              AND s.fhir_version = $2
              AND (s.content->>'derivation' IS NULL OR s.content->>'derivation' != 'constraint')
            ORDER BY s.content->>'name', p.priority DESC
            "#,
        )
        .bind(schema_types)
        .bind(fhir_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(names)
    }

    /// List FHIRSchema names with their source package for categorization.
    ///
    /// Returns tuples of (name, package_name) for resource type categorization.
    /// Used by the Resource Browser UI to group resources by source.
    ///
    /// Categorization:
    /// - `fhir`: package_name starts with `hl7.fhir.`
    /// - `system`: package_name is `octofhir-internal`
    /// - `custom`: all other packages
    #[instrument(skip(self))]
    pub async fn list_fhirschema_names_with_package(
        &self,
        schema_types: &[&str],
        fhir_version: &str,
    ) -> Result<Vec<FhirSchemaInfo>, FcmError> {
        debug!(
            "Listing FHIRSchema names with packages of types: {:?} (FHIR {})",
            schema_types, fhir_version
        );

        let rows = query(
            r#"
            SELECT DISTINCT ON (s.content->>'name')
                s.content->>'name' as name,
                s.content->>'url' as url,
                s.package_name,
                s.package_version
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.schema_type = ANY($1)
              AND s.fhir_version = $2
              AND (s.content->>'derivation' IS NULL OR s.content->>'derivation' != 'constraint')
            ORDER BY s.content->>'name', p.priority DESC
            "#,
        )
        .bind(schema_types)
        .bind(fhir_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let results: Vec<FhirSchemaInfo> = rows
            .iter()
            .filter_map(|r| {
                let name: Option<String> = r.get("name");
                let url: Option<String> = r.get("url");
                let package_name: String = r.get("package_name");
                let package_version: String = r.get("package_version");
                name.map(|n| FhirSchemaInfo {
                    name: n,
                    url,
                    package_name,
                    package_version,
                })
            })
            .collect();

        Ok(results)
    }

    /// Bulk load all FHIRSchemas for a FHIR version.
    ///
    /// Loads all resource and complex-type schemas needed for GraphQL schema building.
    /// Returns the full FhirSchemaRecord with content for each schema.
    ///
    /// This is optimized for the initial GraphQL schema build where all schemas
    /// need to be processed. After building, the returned data can be dropped
    /// to free memory.
    #[instrument(skip(self))]
    pub async fn bulk_load_fhirschemas_for_graphql(
        &self,
        fhir_version: &str,
    ) -> Result<Vec<FhirSchemaRecord>, FcmError> {
        debug!(
            "Bulk loading FHIRSchemas for GraphQL (FHIR {})",
            fhir_version
        );

        // Load all schemas of types needed for GraphQL: resource, complex-type, logical
        // Join with packages to respect priority ordering (higher priority packages first)
        // Use DISTINCT ON to deduplicate by schema name, keeping highest priority
        let rows = query(
            r#"
            SELECT DISTINCT ON (s.content->>'name')
                s.url, s.version, s.package_name, s.package_version,
                s.fhir_version, s.schema_type, s.content, s.content_hash
            FROM fcm.fhirschemas s
            JOIN fcm.packages p ON s.package_name = p.name AND s.package_version = p.version
            WHERE s.fhir_version = $1
              AND s.schema_type IN ('resource', 'complex-type', 'logical')
            ORDER BY s.content->>'name', p.priority DESC
            "#,
        )
        .bind(fhir_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let records: Vec<FhirSchemaRecord> = rows
            .iter()
            .map(|r| FhirSchemaRecord {
                url: r.get("url"),
                version: r.get("version"),
                package_name: r.get("package_name"),
                package_version: r.get("package_version"),
                fhir_version: r.get("fhir_version"),
                schema_type: r.get("schema_type"),
                content: r.get("content"),
                content_hash: r.get("content_hash"),
            })
            .collect();

        info!(
            "Bulk loaded {} FHIRSchemas for GraphQL (FHIR {})",
            records.len(),
            fhir_version
        );

        Ok(records)
    }

    /// Find resources of a specific type from a specific package.
    ///
    /// Returns the raw JSON content of matching resources.
    #[instrument(skip(self))]
    /// Bulk-fetch `content` for every resource of a given resource_type
    /// across all packages, optionally restricted by FHIR version.
    ///
    /// `fhir_versions` accepts the set of strings that should match the
    /// `fcm.resources.fhir_version` column. Pass both the long form
    /// (e.g. `"4.0.1"` written by canonical-manager) and the short form
    /// (e.g. `"R4"` used by our embedded IGs) so a single configured
    /// version picks up both kinds of rows.
    ///
    /// One round-trip replaces the canonical-manager search path
    /// (`get_cache_entries` → in-memory filter → per-match `get_resource`),
    /// which costs 1+N round-trips. Used by the SearchParameter loader on
    /// cold-boot — turning ~1400 SELECTs into one.
    pub async fn find_resources_by_type_bulk(
        &self,
        resource_type: &str,
        fhir_versions: Option<&[&str]>,
    ) -> Result<Vec<Value>, FcmError> {
        let rows: Vec<Value> = match fhir_versions {
            Some(versions) if !versions.is_empty() => {
                let owned: Vec<String> = versions.iter().map(|s| s.to_string()).collect();
                query_scalar(
                    "SELECT content FROM fcm.resources WHERE resource_type = $1 AND fhir_version = ANY($2)",
                )
                .bind(resource_type)
                .bind(&owned)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
            }
            _ => query_scalar("SELECT content FROM fcm.resources WHERE resource_type = $1")
                .bind(resource_type)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?,
        };
        Ok(rows)
    }

    pub async fn find_resources_by_package_and_type(
        &self,
        package_name: &str,
        package_version: &str,
        resource_type: &str,
    ) -> Result<Vec<Value>, FcmError> {
        debug!(
            "Finding {} resources from {}@{}",
            resource_type, package_name, package_version
        );

        let rows: Vec<Value> = query_scalar(
            r#"
            SELECT content
            FROM fcm.resources
            WHERE package_name = $1
              AND package_version = $2
              AND resource_type = $3
            "#,
        )
        .bind(package_name)
        .bind(package_version)
        .bind(resource_type)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        debug!(
            "Found {} {} resources from {}@{}",
            rows.len(),
            resource_type,
            package_name,
            package_version
        );

        Ok(rows)
    }

    /// Get FHIRSchema URLs that are missing for a package.
    ///
    /// Compares the StructureDefinitions in fcm.resources with the
    /// FHIRSchemas in fcm.fhirschemas to find which ones need conversion.
    #[instrument(skip(self))]
    pub async fn find_missing_fhirschemas(
        &self,
        package_name: &str,
        package_version: &str,
    ) -> Result<Vec<String>, FcmError> {
        debug!(
            "Finding missing FHIRSchemas for package {}@{}",
            package_name, package_version
        );

        let urls: Vec<String> = query_scalar(
            r#"
            SELECT r.url
            FROM fcm.resources r
            LEFT JOIN fcm.fhirschemas s
                ON r.url = s.url
                AND r.package_name = s.package_name
                AND r.package_version = s.package_version
            WHERE r.package_name = $1
              AND r.package_version = $2
              AND r.resource_type = 'StructureDefinition'
              AND r.url IS NOT NULL
              AND s.url IS NULL
            ORDER BY r.url
            "#,
        )
        .bind(package_name)
        .bind(package_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        debug!(
            "Found {} StructureDefinitions without FHIRSchemas",
            urls.len()
        );
        Ok(urls)
    }
}

#[async_trait]
impl PackageStore for PostgresPackageStore {
    #[instrument(skip(self, package), fields(name = %package.name, version = %package.version))]
    async fn add_package(
        &self,
        package: &ExtractedPackage,
    ) -> octofhir_canonical_manager::error::Result<()> {
        info!(
            "PostgreSQL storage: add_package called for {}@{} with {} resources",
            package.name,
            package.version,
            package.resources.len()
        );

        let manifest_hash = Self::compute_manifest_hash(package);
        let fhir_version = package
            .manifest
            .fhir_versions
            .as_ref()
            .and_then(|v| v.first().cloned())
            .unwrap_or_else(|| "4.0.1".to_string());

        // Insert package record
        query(
            r#"
            INSERT INTO fcm.packages (name, version, fhir_version, manifest_hash, resource_count, priority)
            VALUES ($1, $2, $3, $4, $5, 0)
            ON CONFLICT (name, version) DO UPDATE SET
                fhir_version = EXCLUDED.fhir_version,
                manifest_hash = EXCLUDED.manifest_hash,
                resource_count = EXCLUDED.resource_count,
                installed_at = NOW()
            "#,
        )
        .bind(&package.name)
        .bind(&package.version)
        .bind(&fhir_version)
        .bind(&manifest_hash)
        .bind(package.resources.len() as i32)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        // Delete existing resources for this package (for re-installation)
        query("DELETE FROM fcm.resources WHERE package_name = $1 AND package_version = $2")
            .bind(&package.name)
            .bind(&package.version)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;

        // Delete existing FHIRSchemas for this package (they will be regenerated on-demand)
        let deleted_schemas =
            query("DELETE FROM fcm.fhirschemas WHERE package_name = $1 AND package_version = $2")
                .bind(&package.name)
                .bind(&package.version)
                .execute(&self.pool)
                .await
                .map_err(db_error)?
                .rows_affected();

        if deleted_schemas > 0 {
            info!(
                "Deleted {} old FHIRSchemas for package {}@{} (will regenerate on-demand)",
                deleted_schemas, package.name, package.version
            );
        }

        // Batch-insert resources via a single UNNEST INSERT. r4.core ships
        // ~4581 resources; sending them one-at-a-time was ~13s of cold-boot
        // — almost all of that is per-statement RTT, not Postgres CPU. The
        // ON CONFLICT clause makes the batch idempotent against re-runs.
        let n = package.resources.len();
        let mut resource_types: Vec<String> = Vec::with_capacity(n);
        let mut resource_ids: Vec<Option<String>> = Vec::with_capacity(n);
        let mut urls: Vec<Option<String>> = Vec::with_capacity(n);
        let mut names: Vec<Option<String>> = Vec::with_capacity(n);
        let mut versions: Vec<Option<String>> = Vec::with_capacity(n);
        let mut sd_kinds: Vec<Option<String>> = Vec::with_capacity(n);
        let mut sd_derivations: Vec<Option<String>> = Vec::with_capacity(n);
        let mut sd_types: Vec<Option<String>> = Vec::with_capacity(n);
        let mut sd_base_defs: Vec<Option<String>> = Vec::with_capacity(n);
        let mut sd_abstracts: Vec<Option<bool>> = Vec::with_capacity(n);
        let mut sd_impose: Vec<Option<Value>> = Vec::with_capacity(n);
        let mut sd_chars: Vec<Option<Value>> = Vec::with_capacity(n);
        let mut sd_flavors: Vec<Option<String>> = Vec::with_capacity(n);
        let mut content_hashes: Vec<String> = Vec::with_capacity(n);
        let mut contents: Vec<Value> = Vec::with_capacity(n);

        for resource in &package.resources {
            let content = &resource.content;
            let sd_fields = Self::extract_sd_fields(content);
            resource_types.push(resource.resource_type.clone());
            resource_ids.push(Some(resource.id.clone()));
            urls.push(resource.url.clone());
            names.push(
                content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            );
            versions.push(resource.version.clone());
            sd_kinds.push(sd_fields.kind);
            sd_derivations.push(sd_fields.derivation);
            sd_types.push(sd_fields.sd_type);
            sd_base_defs.push(sd_fields.base_definition);
            sd_abstracts.push(sd_fields.is_abstract);
            sd_impose.push(
                sd_fields
                    .impose_profiles
                    .and_then(|v| serde_json::to_value(v).ok()),
            );
            sd_chars.push(
                sd_fields
                    .characteristics
                    .and_then(|v| serde_json::to_value(v).ok()),
            );
            sd_flavors.push(sd_fields.flavor);

            // fcm.resources.content_hash is write-only here — never read on
            // subsequent boots. Avoid the per-resource `serde_json::to_string`
            // which on cold-boot dominated CPU (4581 R4 resources × full
            // serialize). Hash only the stable identity (resource_type / id /
            // url) instead.
            let hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                resource.resource_type.hash(&mut hasher);
                resource.id.hash(&mut hasher);
                resource.url.hash(&mut hasher);
                resource.version.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            };
            content_hashes.push(hash);
            contents.push(content.clone());
        }

        if n > 0 {
            query(
                r#"
                INSERT INTO fcm.resources (
                    resource_type, resource_id, url, name, version,
                    sd_kind, sd_derivation, sd_type, sd_base_definition, sd_abstract,
                    sd_impose_profiles, sd_characteristics, sd_flavor,
                    package_name, package_version, fhir_version, content_hash, content
                )
                SELECT
                    t.rt, t.rid, t.url, t.nm, t.ver,
                    t.kind, t.deriv, t.styp, t.bdef, t.abs_,
                    t.imp, t.chr, t.flav,
                    $14, $15, $16, t.hsh, t.cnt
                FROM UNNEST(
                    $1::text[], $2::text[], $3::text[], $4::text[], $5::text[],
                    $6::text[], $7::text[], $8::text[], $9::text[], $10::bool[],
                    $11::jsonb[], $12::jsonb[], $13::text[],
                    $17::text[], $18::jsonb[]
                ) AS t(rt, rid, url, nm, ver, kind, deriv, styp, bdef, abs_, imp, chr, flav, hsh, cnt)
                "#,
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&urls)
            .bind(&names)
            .bind(&versions)
            .bind(&sd_kinds)
            .bind(&sd_derivations)
            .bind(&sd_types)
            .bind(&sd_base_defs)
            .bind(&sd_abstracts)
            .bind(&sd_impose)
            .bind(&sd_chars)
            .bind(&sd_flavors)
            .bind(&package.name)
            .bind(&package.version)
            .bind(&fhir_version)
            .bind(&content_hashes)
            .bind(&contents)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        }

        info!(
            "PostgreSQL storage: successfully added package {}@{} with {} resources to database",
            package.name,
            package.version,
            package.resources.len()
        );

        Ok(())
    }

    #[instrument(skip(self))]
    async fn remove_package(
        &self,
        name: &str,
        version: &str,
    ) -> octofhir_canonical_manager::error::Result<bool> {
        info!("Removing package {}@{}", name, version);

        let result = query("DELETE FROM fcm.packages WHERE name = $1 AND version = $2")
            .bind(name)
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;

        let removed = result.rows_affected() > 0;

        if removed {
            info!("Removed package {}@{}", name, version);
        } else {
            warn!("Package {}@{} not found for removal", name, version);
        }

        Ok(removed)
    }

    #[instrument(skip(self))]
    async fn find_resource(
        &self,
        canonical_url: &str,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        debug!("Finding resource by canonical URL: {}", canonical_url);

        let sql = format!(
            "{} WHERE url = $1 OR url_lower = lower($1) LIMIT 1",
            Self::RESOURCE_SELECT
        );

        let row = query(&sql)
            .bind(canonical_url)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(row.as_ref().map(row_to_resource_index))
    }

    #[instrument(skip(self))]
    async fn list_packages(&self) -> octofhir_canonical_manager::error::Result<Vec<PackageInfo>> {
        debug!("Listing all packages");

        let rows = query(
            r#"
            SELECT name, version, fhir_version, installed_at, resource_count
            FROM fcm.packages
            ORDER BY name, version
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows
            .iter()
            .map(|row| {
                let count: i32 = row.get("resource_count");
                PackageInfo {
                    name: row.get("name"),
                    version: row.get("version"),
                    fhir_version: row
                        .get::<Option<String>, _>("fhir_version")
                        .unwrap_or_else(|| "4.0.1".to_string()),
                    installed_at: row.get("installed_at"),
                    resource_count: count as usize,
                }
            })
            .collect())
    }
}

#[async_trait]
impl SearchStorage for PostgresPackageStore {
    async fn find_resource(
        &self,
        canonical_url: &str,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        PackageStore::find_resource(self, canonical_url).await
    }

    async fn find_resource_with_fhir_version(
        &self,
        canonical_url: &str,
        fhir_version: &str,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        debug!(
            "Finding resource {} with FHIR version {}",
            canonical_url, fhir_version
        );

        let sql = format!(
            "{} WHERE (url = $1 OR url_lower = lower($1)) AND fhir_version = $2 LIMIT 1",
            Self::RESOURCE_SELECT
        );

        let row = query(&sql)
            .bind(canonical_url)
            .bind(fhir_version)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(row.as_ref().map(row_to_resource_index))
    }

    async fn find_by_base_url(
        &self,
        base_url: &str,
    ) -> octofhir_canonical_manager::error::Result<Vec<ResourceIndex>> {
        debug!("Finding resources by base URL: {}", base_url);

        let pattern = format!("{}%", base_url.trim_end_matches('/'));

        let sql = format!(
            "{} WHERE url LIKE $1 ORDER BY version DESC NULLS LAST",
            Self::RESOURCE_SELECT
        );

        let rows = query(&sql)
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(rows.iter().map(row_to_resource_index).collect())
    }

    async fn find_latest_by_base_url(
        &self,
        base_url: &str,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        let results = self.find_by_base_url(base_url).await?;
        Ok(results.into_iter().next())
    }

    async fn find_resource_by_name(
        &self,
        name: &str,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        debug!("Finding resource by name: {}", name);

        let sql = format!(
            "{} WHERE name_lower = lower($1) LIMIT 1",
            Self::RESOURCE_SELECT
        );

        let row = query(&sql)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(row.as_ref().map(row_to_resource_index))
    }

    async fn find_by_type_and_id(
        &self,
        resource_type: String,
        id: String,
    ) -> octofhir_canonical_manager::error::Result<Vec<ResourceIndex>> {
        debug!("Finding resources by type {} and id {}", resource_type, id);

        let sql = format!(
            "{} WHERE resource_type = $1 AND id_lower = lower($2)",
            Self::RESOURCE_SELECT
        );

        let rows = query(&sql)
            .bind(&resource_type)
            .bind(&id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(rows.iter().map(row_to_resource_index).collect())
    }

    async fn find_by_type_and_name(
        &self,
        resource_type: String,
        name: String,
    ) -> octofhir_canonical_manager::error::Result<Vec<ResourceIndex>> {
        debug!(
            "Finding resources by type {} and name {}",
            resource_type, name
        );

        let sql = format!(
            "{} WHERE resource_type = $1 AND name_lower = lower($2)",
            Self::RESOURCE_SELECT
        );

        let rows = query(&sql)
            .bind(&resource_type)
            .bind(&name)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(rows.iter().map(row_to_resource_index).collect())
    }

    async fn find_resource_info(
        &self,
        key: &str,
        types: Option<&[&str]>,
        _exclude_extensions: bool,
        _sort_by_priority: bool,
    ) -> octofhir_canonical_manager::error::Result<Option<ResourceIndex>> {
        let results = self.find_resource_infos(key, types, Some(1)).await?;
        Ok(results.into_iter().next())
    }

    async fn find_resource_infos(
        &self,
        key: &str,
        types: Option<&[&str]>,
        limit: Option<usize>,
    ) -> octofhir_canonical_manager::error::Result<Vec<ResourceIndex>> {
        debug!("Finding resource infos for key: {}", key);

        let mut sql = format!(
            "{} WHERE (url = $1 OR url_lower = lower($1) OR name_lower = lower($1) OR id_lower = lower($1))",
            Self::RESOURCE_SELECT
        );

        if let Some(type_list) = types
            && !type_list.is_empty()
        {
            let types_str = type_list
                .iter()
                .map(|t| format!("'{}'", t))
                .collect::<Vec<_>>()
                .join(",");
            sql.push_str(&format!(" AND resource_type IN ({})", types_str));
        }

        sql.push_str(" ORDER BY package_name, package_version DESC");

        if let Some(lim) = limit {
            sql.push_str(&format!(" LIMIT {}", lim));
        }

        let rows = query(&sql)
            .bind(key)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(rows.iter().map(row_to_resource_index).collect())
    }

    async fn list_base_resource_type_names(
        &self,
        fhir_version: &str,
    ) -> octofhir_canonical_manager::error::Result<Vec<String>> {
        debug!("Listing base resource type names for FHIR {}", fhir_version);

        let names: Vec<String> = query_scalar(
            r#"
            SELECT DISTINCT sd_type
            FROM fcm.resources
            WHERE resource_type = 'StructureDefinition'
              AND sd_kind = 'resource'
              AND sd_derivation = 'specialization'
              AND fhir_version = $1
              AND sd_type IS NOT NULL
            ORDER BY sd_type
            "#,
        )
        .bind(fhir_version)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(names)
    }

    async fn get_resource(
        &self,
        resource_index: &ResourceIndex,
    ) -> octofhir_canonical_manager::error::Result<FhirResource> {
        debug!(
            "Getting resource content for: {}",
            resource_index.canonical_url
        );

        let row = query(
            r#"
            SELECT resource_type, resource_id, url, version, content
            FROM fcm.resources
            WHERE url = $1 AND package_name = $2 AND package_version = $3
            "#,
        )
        .bind(&resource_index.canonical_url)
        .bind(&resource_index.package_name)
        .bind(&resource_index.package_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        match row {
            Some(r) => {
                let resource_id: Option<String> = r.get("resource_id");
                Ok(FhirResource {
                    resource_type: r.get("resource_type"),
                    id: resource_id.unwrap_or_default(),
                    url: r.get("url"),
                    version: r.get("version"),
                    content: r.get("content"),
                    file_path: PathBuf::new(),
                })
            }
            None => Err(FcmError::Storage(StorageError::ResourceNotFound {
                canonical_url: resource_index.canonical_url.clone(),
            })),
        }
    }

    async fn get_cache_entries(&self) -> Vec<ResourceIndex> {
        debug!("Getting all cache entries");

        let sql = format!("{} WHERE url IS NOT NULL", Self::RESOURCE_SELECT);

        let rows = match query(&sql).fetch_all(&self.pool).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!("Failed to get cache entries: {}", e);
                return Vec::new();
            }
        };

        rows.iter()
            .filter_map(|row| {
                let url: Option<String> = row.get("url");
                if url.is_some() && !url.as_ref().unwrap().is_empty() {
                    Some(row_to_resource_index(row))
                } else {
                    None
                }
            })
            .collect()
    }

    async fn find_by_type_and_package(
        &self,
        resource_type: &str,
        package_name: &str,
    ) -> octofhir_canonical_manager::error::Result<Vec<ResourceIndex>> {
        debug!(
            "Finding resources by type {} and package {}",
            resource_type, package_name
        );

        let sql = format!(
            "{} WHERE resource_type = $1 AND package_name = $2",
            Self::RESOURCE_SELECT
        );

        let rows = query(&sql)
            .bind(resource_type)
            .bind(package_name)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(rows.iter().map(row_to_resource_index).collect())
    }

    async fn list_packages(&self) -> octofhir_canonical_manager::error::Result<Vec<PackageInfo>> {
        PackageStore::list_packages(self).await
    }

    async fn set_package_priority(
        &self,
        package_name: &str,
        package_version: &str,
        priority: i32,
    ) -> octofhir_canonical_manager::error::Result<()> {
        debug!(
            "Setting priority {} for package {}@{}",
            priority, package_name, package_version
        );

        query(
            r#"
            UPDATE fcm.packages
            SET priority = $3
            WHERE name = $1 AND version = $2
            "#,
        )
        .bind(package_name)
        .bind(package_version)
        .bind(priority)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sd_flavor_determination() {
        let content = serde_json::json!({
            "kind": "resource",
            "derivation": "specialization"
        });
        assert_eq!(
            PostgresPackageStore::determine_sd_flavor(&content),
            Some("resource".to_string())
        );

        let content = serde_json::json!({
            "kind": "resource",
            "derivation": "constraint"
        });
        assert_eq!(
            PostgresPackageStore::determine_sd_flavor(&content),
            Some("profile".to_string())
        );

        let content = serde_json::json!({
            "kind": "complex-type",
            "derivation": "constraint",
            "type": "Extension"
        });
        assert_eq!(
            PostgresPackageStore::determine_sd_flavor(&content),
            Some("extension".to_string())
        );

        let content = serde_json::json!({
            "kind": "primitive-type"
        });
        assert_eq!(
            PostgresPackageStore::determine_sd_flavor(&content),
            Some("primitive-type".to_string())
        );
    }
}
