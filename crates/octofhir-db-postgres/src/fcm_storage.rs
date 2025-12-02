//! PostgreSQL implementation of the PackageStore and SearchStorage traits
//! from octofhir-canonical-manager.
//!
//! This module provides a PostgreSQL backend for storing FHIR packages and resources
//! from Implementation Guides, enabling efficient querying and resolution of
//! canonical URLs in a server environment.

use std::collections::HashMap;
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
        canonical_url: url.clone().unwrap_or_default(),
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

    /// Common SQL for selecting resource fields
    const RESOURCE_SELECT: &'static str = r#"
        SELECT
            resource_type, resource_id, url, name, version,
            package_name, package_version, fhir_version, content_hash,
            sd_kind, sd_derivation, sd_type, sd_base_definition, sd_abstract,
            sd_impose_profiles, sd_characteristics, sd_flavor
        FROM fcm.resources
    "#;
}

#[async_trait]
impl PackageStore for PostgresPackageStore {
    #[instrument(skip(self, package), fields(name = %package.name, version = %package.version))]
    async fn add_package(
        &self,
        package: &ExtractedPackage,
    ) -> octofhir_canonical_manager::error::Result<()> {
        info!(
            "Adding package {}@{} with {} resources",
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

        // Insert resources
        for resource in &package.resources {
            let content = &resource.content;
            let sd_fields = Self::extract_sd_fields(content);

            // Compute content hash
            let content_str = serde_json::to_string(content).unwrap_or_default();
            let content_hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                content_str.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            };

            query(
                r#"
                INSERT INTO fcm.resources (
                    resource_type, resource_id, url, name, version,
                    sd_kind, sd_derivation, sd_type, sd_base_definition, sd_abstract,
                    sd_impose_profiles, sd_characteristics, sd_flavor,
                    package_name, package_version, fhir_version, content_hash, content
                ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13,
                    $14, $15, $16, $17, $18
                )
                "#,
            )
            .bind(&resource.resource_type)
            .bind(&resource.id)
            .bind(&resource.url)
            .bind(content.get("name").and_then(|v| v.as_str()))
            .bind(&resource.version)
            .bind(&sd_fields.kind)
            .bind(&sd_fields.derivation)
            .bind(&sd_fields.sd_type)
            .bind(&sd_fields.base_definition)
            .bind(sd_fields.is_abstract)
            .bind(
                sd_fields
                    .impose_profiles
                    .and_then(|v| serde_json::to_value(v).ok()),
            )
            .bind(
                sd_fields
                    .characteristics
                    .and_then(|v| serde_json::to_value(v).ok()),
            )
            .bind(&sd_fields.flavor)
            .bind(&package.name)
            .bind(&package.version)
            .bind(&fhir_version)
            .bind(&content_hash)
            .bind(content)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        }

        info!(
            "Successfully added package {}@{} with {} resources",
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
            SELECT name, version, installed_at, resource_count
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

    async fn get_cache_entries(&self) -> HashMap<String, ResourceIndex> {
        debug!("Getting all cache entries");

        let sql = format!("{} WHERE url IS NOT NULL", Self::RESOURCE_SELECT);

        let rows = match query(&sql).fetch_all(&self.pool).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!("Failed to get cache entries: {}", e);
                return HashMap::new();
            }
        };

        let mut cache = HashMap::new();
        for row in &rows {
            let url: Option<String> = row.get("url");
            if let Some(url) = url
                && !url.is_empty()
            {
                cache.insert(url, row_to_resource_index(row));
            }
        }

        cache
    }

    async fn list_packages(&self) -> octofhir_canonical_manager::error::Result<Vec<PackageInfo>> {
        PackageStore::list_packages(self).await
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
