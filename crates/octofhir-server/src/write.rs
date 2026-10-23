//! Atomic primary writes and speculative write validation share the same locks,
//! trusted grants, policies, version conditions and reference integrity checks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::http::{HeaderMap, header};
use octofhir_api::ApiError;
use octofhir_auth::{
    policy::{AccessDecision, PolicyContext, ResourceContext},
    smart::scopes::FhirOperation,
};
use octofhir_storage::{
    RawStoredResource, SearchParams, StorageError, StoredResource, Transaction,
};
use serde_json::Value;

use crate::{handlers::LocalReferenceSet, server::AppState};

/// Only trusted authorization code can construct this request extension. Actual
/// resource authorization is deferred to the transaction that performs the write.
#[derive(Clone, Debug)]
pub struct AuthorizedWrite(pub(crate) PolicyContext);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum WriteMode {
    Create,
    Update,
    Delete,
}

impl WriteMode {
    pub(crate) fn operation(self) -> FhirOperation {
        match self {
            Self::Create => FhirOperation::Create,
            Self::Update => FhirOperation::Update,
            Self::Delete => FhirOperation::Delete,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WriteError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("Required references are unavailable")]
    MissingReferences(HashSet<String>),
}

pub(crate) fn parse_if_match(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    let version = value
        .strip_prefix("W/\"")
        .or_else(|| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| ApiError::bad_request("If-Match requires a single version ETag"))?;
    octofhir_core::validate_id(version)
        .map_err(|_| ApiError::bad_request("Invalid If-Match version ETag"))?;
    Ok(version.to_owned())
}

pub(crate) fn if_match_header(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let mut values = headers.get_all(header::IF_MATCH).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(ApiError::bad_request(
            "If-Match requires a single version ETag",
        ));
    }
    value
        .map(|value| {
            parse_if_match(
                value
                    .to_str()
                    .map_err(|_| ApiError::bad_request("Invalid If-Match header"))?,
            )
        })
        .transpose()
}

pub(crate) struct WriteResult {
    pub(crate) stored: Option<StoredResource>,
    pub(crate) previous: Option<StoredResource>,
    pub(crate) deletion: Option<octofhir_storage::DeletedResource>,
    pub(crate) created: bool,
}

impl WriteResult {
    pub(crate) fn into_raw(self) -> RawStoredResource {
        let stored = self
            .stored
            .expect("create/update returns a stored resource");
        RawStoredResource {
            resource_json: stored.resource.to_string(),
            id: stored.id,
            version_id: stored.version_id,
            resource_type: stored.resource_type,
            created_at: stored.created_at,
            last_updated: stored.last_updated,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PreparedWrite {
    mode: WriteMode,
    resource_type: String,
    id: Option<String>,
    policy: Option<PolicyContext>,
    params: SearchParams,
    constrained: bool,
}

impl PreparedWrite {
    /// Resolve external terminology before acquiring the write connection.
    pub(crate) async fn prepare(
        state: &AppState,
        mode: WriteMode,
        resource_type: &str,
        id: Option<&str>,
        policy: Option<PolicyContext>,
    ) -> Result<Self, ApiError> {
        if state.storage.backend_name() != "postgres" {
            return Err(ApiError::not_implemented(
                "Atomic writes require transaction row locking",
            ));
        }
        let id = if mode == WriteMode::Create { None } else { id };
        if let Some(id) = id {
            octofhir_core::validate_id(id)
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
        }
        let mut params = SearchParams::new().with_count(1);
        if let Some(id) = id {
            params = params.with_param("_id", id);
        }
        if let Some(policy) = &policy {
            if !policy.scopes.parsed.permits(
                resource_type,
                mode.operation(),
                policy.environment.patient_context.as_deref(),
            ) {
                return Err(ApiError::forbidden(
                    "Insufficient scope for this interaction",
                ));
            }
            params.constraint = crate::authorization::query_visibility(
                policy,
                &state.compartment_registry,
                &state.base_url,
            )?;
        }
        let constrained = params.constraint.is_some();
        let cfg = state.search_config.config();
        let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
            &[resource_type],
            &params,
            &cfg.registry,
            state.terminology_provider.as_ref(),
            state.config.search.max_valueset_expansion,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        Ok(Self {
            mode,
            resource_type: resource_type.into(),
            id: id.map(String::from),
            policy,
            params,
            constrained,
        })
    }

    pub(crate) async fn run(
        &self,
        state: &AppState,
        candidate: Option<&Value>,
        references: &LocalReferenceSet,
        if_match: Option<&str>,
        commit: bool,
    ) -> Result<WriteResult, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = self
            .apply(state, &mut *tx, candidate, references, if_match)
            .await;
        if commit && result.is_ok() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }

    pub(crate) async fn conditional(
        &self,
        state: &AppState,
        query: &str,
    ) -> Result<PreparedConditionalWrite<'_>, ApiError> {
        let mut params = crate::handlers::conditional_search_params(query);
        if let Some(policy) = &self.policy {
            params.constraint = crate::authorization::query_visibility(
                policy,
                &state.compartment_registry,
                &state.base_url,
            )?;
        }
        let cfg = state.search_config.config();
        let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
            &[self.resource_type.as_str()],
            &params,
            &cfg.registry,
            state.terminology_provider.as_ref(),
            state.config.search.max_valueset_expansion,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        Ok(PreparedConditionalWrite {
            write: self,
            params,
        })
    }

    pub(crate) fn resolved(&self, id: Option<&str>) -> Self {
        let mut resolved = self.clone();
        resolved.id = id.map(str::to_owned);
        if self.mode != WriteMode::Create
            && let Some(policy) = &mut resolved.policy
        {
            policy.request.resource_id = resolved.id.clone();
        }
        resolved
    }

    /// Read a grant-visible snapshot without retaining a pool connection during
    /// patch evaluation and schema loading. The caller must authorize its patch
    /// result before exposing errors and pin this version in the final write.
    pub(crate) async fn snapshot(&self, state: &AppState) -> Result<StoredResource, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = self
            .read_previous(state, &mut *tx)
            .await
            .and_then(|previous| {
                previous.ok_or_else(|| {
                    ApiError::not_found("Resource is unavailable for this write").into()
                })
            });
        tx.rollback().await?;
        result
    }

    pub(crate) async fn authorize_snapshot(
        &self,
        state: &AppState,
        previous: &StoredResource,
        candidate: Option<&Value>,
    ) -> Result<(), WriteError> {
        self.authorize_resource(state, &previous.id, &previous.resource, candidate)
            .await
    }

    async fn authorize_resource(
        &self,
        state: &AppState,
        id: &str,
        resource: &Value,
        candidate: Option<&Value>,
    ) -> Result<(), WriteError> {
        if let Some(mut policy) = self.policy.clone() {
            policy.request.body = candidate.cloned();
            if self.mode != WriteMode::Create {
                policy.request.resource_id = Some(id.into());
            }
            policy.resource = Some(ResourceContext::from_resource(resource.clone()));
            Self::evaluate(state, &policy).await?;
        }
        Ok(())
    }

    async fn visible(
        &self,
        state: &AppState,
        tx: &dyn Transaction,
        stored: &StoredResource,
    ) -> Result<bool, WriteError> {
        if !self.constrained {
            return Ok(true);
        }
        let mut params = self.params.clone();
        params
            .parameters
            .insert("_id".into(), vec![stored.id.clone()]);
        let visible = tx
            .search_system_raw(
                &[self.resource_type.as_str()],
                &params,
                state.config.search.include_limits,
                false,
            )
            .await?;
        Ok(visible
            .entries
            .iter()
            .any(|row| row.id == stored.id && row.version_id == stored.version_id))
    }

    async fn evaluate(state: &AppState, policy: &PolicyContext) -> Result<(), WriteError> {
        match state.policy_evaluator.evaluate(policy).await {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message).into()),
            AccessDecision::Abstain => {
                Err(ApiError::forbidden("No policy allows this write").into())
            }
        }
    }

    async fn read_previous(
        &self,
        state: &AppState,
        tx: &mut dyn Transaction,
    ) -> Result<Option<StoredResource>, WriteError> {
        let previous = if let Some(id) = &self.id {
            match tx.read_for_update(&self.resource_type, id).await {
                Err(StorageError::Deleted { .. }) if self.constrained => {
                    return Err(
                        ApiError::not_found("Resource is unavailable for this write").into(),
                    );
                }
                result => result?,
            }
        } else {
            None
        };
        if let Some(previous) = &previous
            && !self.visible(state, tx, previous).await?
        {
            return Err(ApiError::not_found("Resource is unavailable for this write").into());
        }
        Ok(previous)
    }

    async fn apply(
        &self,
        state: &AppState,
        tx: &mut dyn Transaction,
        candidate: Option<&Value>,
        references: &LocalReferenceSet,
        if_match: Option<&str>,
    ) -> Result<WriteResult, WriteError> {
        let previous = self.read_previous(state, tx).await?;
        if previous.is_none()
            && self.constrained
            && (self.mode == WriteMode::Delete || if_match.is_some())
        {
            return Err(ApiError::not_found("Resource is unavailable for this write").into());
        }

        let mut policy = self.policy.clone();
        if let Some(policy) = &mut policy {
            policy.request.body = candidate.cloned();
            policy.resource = previous
                .as_ref()
                .map(|row| ResourceContext::from_resource(row.resource.clone()));
            if previous.is_some() || self.mode == WriteMode::Delete {
                Self::evaluate(state, policy).await?;
            }
        }
        if self.resource_type == "AccessPolicy"
            && self.id.as_deref() == Some(crate::bootstrap::ADMIN_ACCESS_POLICY_ID)
        {
            return Err(
                ApiError::forbidden("The default admin access policy cannot be changed").into(),
            );
        }
        if let Some(expected) = if_match {
            let previous = previous.as_ref().ok_or_else(|| {
                ApiError::precondition_failed("Resource does not exist but If-Match was provided")
            })?;
            if previous.version_id != expected {
                return Err(StorageError::version_conflict(expected, &previous.version_id).into());
            }
        }
        if self.mode == WriteMode::Delete {
            let deletion = if let Some(previous) = &previous {
                let mut deleted = tx
                    .delete_batch_with_snapshots(
                        &self.resource_type,
                        std::slice::from_ref(&previous.id),
                    )
                    .await?;
                let deleted = deleted.pop().ok_or_else(|| {
                    ApiError::conflict("Locked resource disappeared during deletion")
                })?;
                if deleted.previous.resource != previous.resource {
                    return Err(ApiError::conflict(
                        "Deletion snapshot changed after authorization",
                    )
                    .into());
                }
                Some(deleted)
            } else {
                None
            };
            return Ok(WriteResult {
                stored: None,
                previous,
                deletion,
                created: false,
            });
        }
        let candidate =
            candidate.ok_or_else(|| ApiError::bad_request("Write requires a resource"))?;
        let created = previous.is_none();
        let pending_self = (created && self.mode == WriteMode::Update)
            .then(|| {
                self.id
                    .as_ref()
                    .map(|id| format!("{}/{id}", self.resource_type))
            })
            .flatten()
            .filter(|_| candidate["id"].as_str() == self.id.as_deref());
        let mut snapshot_targets = references.versions.clone();
        snapshot_targets.extend(
            references
                .snapshots
                .iter()
                .map(|(reference, _)| reference.clone()),
        );
        snapshot_targets.sort_by_key(octofhir_core::FhirReference::to_versioned);
        snapshot_targets.dedup();
        let mut snapshots = std::collections::HashMap::new();
        if !references.groups.is_empty() || !snapshot_targets.is_empty() {
            let mut found = if references.groups.is_empty() {
                HashSet::new()
            } else {
                tx.lock_references(&references.groups).await?
            };
            if !snapshot_targets.is_empty() {
                snapshots = tx.lock_reference_snapshots(&snapshot_targets).await?;
                found.extend(
                    references
                        .versions
                        .iter()
                        .map(octofhir_core::FhirReference::to_versioned)
                        .filter(|key| snapshots.contains_key(key)),
                );
            }
            // PUT can create its own known path identity. Only that exact
            // unversioned self-reference may await the following insert.
            if let Some(key) = &pending_self
                && references.all.contains(key)
            {
                found.insert(key.clone());
            }
            if found != references.all {
                return Err(WriteError::MissingReferences(found));
            }
        }
        for (reference, expected) in &references.snapshots {
            if snapshots.get(&reference.to_versioned()) != expected.as_deref() {
                return Err(ApiError::conflict(
                    "A reference target changed during validation; retry the write",
                )
                .into());
            }
        }
        let stored = if created {
            tx.create(candidate).await?
        } else {
            tx.update(
                candidate,
                previous.as_ref().map(|row| row.version_id.as_str()),
            )
            .await?
        };
        if let Some(key) = &pending_self
            && references.all.contains(key)
            && key != &format!("{}/{}", stored.resource_type, stored.id)
        {
            return Err(ApiError::conflict("The stored resource identity changed").into());
        }
        if !self.visible(state, tx, &stored).await? {
            return Err(if self.mode == WriteMode::Update && created {
                ApiError::not_found("Resource is unavailable for this write")
            } else {
                ApiError::forbidden("Proposed resource is outside the granted scope")
            }
            .into());
        }
        if let Some(policy) = &mut policy {
            if self.mode != WriteMode::Create {
                policy.request.resource_id = Some(stored.id.clone());
            }
            policy.resource = Some(ResourceContext::from_resource(stored.resource.clone()));
            Self::evaluate(state, policy).await?;
        }
        Ok(WriteResult {
            deletion: None,
            stored: Some(stored),
            previous,
            created,
        })
    }
}

/// One authenticated caller owns every entry. Prepare its grant predicates once
/// per type/permission, lock previous bodies by type, and retain native batch
/// mutations. Policies still receive each entry's actual path and proposed body.
#[derive(Default)]
pub(crate) struct PreparedTransactionWrites {
    writes: HashMap<usize, PreparedWrite>,
}

impl PreparedTransactionWrites {
    pub(crate) async fn prepare(
        state: &AppState,
        contexts: HashMap<usize, PolicyContext>,
    ) -> Result<Self, ApiError> {
        let mut templates = HashMap::new();
        let mut writes = HashMap::new();
        for (index, mut context) in contexts {
            // Resolved candidates are supplied at policy checks; retaining the
            // original body would copy it again each time the context is cloned.
            context.request.body = None;
            context.resource = None;
            let mode = match context.request.operation {
                FhirOperation::Create => WriteMode::Create,
                FhirOperation::Update | FhirOperation::Patch => WriteMode::Update,
                FhirOperation::Delete => WriteMode::Delete,
                _ => return Err(ApiError::bad_request("Unsupported transaction write")),
            };
            let key = (context.request.resource_type.clone(), mode);
            if !templates.contains_key(&key) {
                let mut template =
                    PreparedWrite::prepare(state, mode, &key.0, None, Some(context.clone()))
                        .await?;
                // The template holds grants, not the first entry's body.
                template.policy = None;
                templates.insert(key.clone(), template);
            }
            let mut write = templates[&key].resolved(context.request.resource_id.as_deref());
            write.policy = Some(context);
            writes.insert(index, write);
        }
        Ok(Self { writes })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&PreparedWrite> {
        self.writes.get(&index)
    }

    pub(crate) fn bind_target(&mut self, index: usize, id: Option<&str>) {
        if let Some(write) = self.writes.get_mut(&index) {
            *write = write.resolved(id);
        }
    }

    async fn visible_group(
        state: &AppState,
        tx: &dyn Transaction,
        write: &PreparedWrite,
        rows: &[(&str, &str)],
    ) -> Result<Option<HashMap<String, String>>, WriteError> {
        if !write.constrained || rows.is_empty() {
            return Ok(None);
        }
        let ids: Vec<_> = rows.iter().map(|(id, _)| (*id).to_owned()).collect();
        let mut found = HashMap::new();
        // This trusted check is bounded independently of the public page limit.
        // No entry causes its own round trip; large imports use native chunks.
        let chunk_size = state.config.search.max_count.clamp(1, 1024);
        for chunk in ids.chunks(chunk_size) {
            let mut params = write.params.clone().with_count(chunk.len() as u32);
            params
                .parameters
                .insert("_id".into(), vec![chunk.join(",")]);
            let result = tx
                .search_system_raw(
                    &[write.resource_type.as_str()],
                    &params,
                    state.config.search.include_limits,
                    false,
                )
                .await?;
            found.extend(
                result
                    .entries
                    .into_iter()
                    .map(|row| (row.id, row.version_id)),
            );
        }
        Ok(Some(found))
    }

    /// Return entry indices that had a live previous row, for masked upsert
    /// failures after checking the actual stored result against its grant.
    pub(crate) async fn authorize_previous(
        &self,
        state: &AppState,
        tx: &dyn Transaction,
        entries: &[(usize, Value)],
        patches: &HashMap<usize, (Value, String)>,
    ) -> Result<HashSet<usize>, WriteError> {
        let mut ids_by_type: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (index, _) in entries {
            if let Some(write) = self.writes.get(index)
                && let Some(id) = &write.id
            {
                ids_by_type
                    .entry(&write.resource_type)
                    .or_default()
                    .push(id.clone());
            }
        }
        let mut previous = HashMap::new();
        for (resource_type, mut ids) in ids_by_type {
            ids.sort();
            ids.dedup();
            for row in tx.read_many_for_update(resource_type, &ids).await? {
                previous.insert((resource_type.to_owned(), row.id.clone()), row);
            }
        }
        let mut groups: BTreeMap<(&str, WriteMode), Vec<(usize, &PreparedWrite, &StoredResource)>> =
            BTreeMap::new();
        for (index, _) in entries {
            let Some(write) = self.writes.get(index) else {
                continue;
            };
            if let Some(id) = &write.id
                && let Some(row) = previous.get(&(write.resource_type.clone(), id.clone()))
            {
                groups
                    .entry((&write.resource_type, write.mode))
                    .or_default()
                    .push((*index, write, row));
            }
        }
        for group in groups.values() {
            let rows: Vec<_> = group
                .iter()
                .map(|(_, _, row)| (row.id.as_str(), row.version_id.as_str()))
                .collect();
            let visible = Self::visible_group(state, tx, group[0].1, &rows).await?;
            if rows.iter().any(|(id, version)| {
                visible.as_ref().is_some_and(|found| {
                    found
                        .get(*id)
                        .is_none_or(|actual| actual.as_str() != *version)
                })
            }) {
                return Err(ApiError::not_found("Resource is unavailable for this write").into());
            }
        }
        let mut existed = HashSet::new();
        for (index, entry) in entries {
            let Some(write) = self.writes.get(index) else {
                continue;
            };
            let row = write
                .id
                .as_ref()
                .and_then(|id| previous.get(&(write.resource_type.clone(), id.clone())));
            let candidate = patches
                .get(index)
                .map(|(body, _)| body)
                .or_else(|| entry.get("resource"));
            if let Some(row) = row {
                existed.insert(*index);
                let candidate = if write.mode == WriteMode::Create {
                    Some(&row.resource)
                } else {
                    candidate
                };
                write.authorize_snapshot(state, row, candidate).await?;
            } else {
                if write.constrained && write.mode == WriteMode::Delete && write.id.is_some() {
                    return Err(
                        ApiError::not_found("Resource is unavailable for this write").into(),
                    );
                }
                if write.mode == WriteMode::Delete
                    && let Some(mut policy) = write.policy.clone()
                {
                    policy.request.body = None;
                    policy.resource = None;
                    PreparedWrite::evaluate(state, &policy).await?;
                }
            }
            if write.mode != WriteMode::Create
                && write.resource_type == "AccessPolicy"
                && write.id.as_deref() == Some(crate::bootstrap::ADMIN_ACCESS_POLICY_ID)
            {
                return Err(ApiError::forbidden(
                    "The default admin access policy cannot be changed",
                )
                .into());
            }
            let supplied = entry["request"]
                .get("ifMatch")
                .and_then(Value::as_str)
                .map(parse_if_match)
                .transpose()?;
            let expected = supplied
                .as_deref()
                .or_else(|| patches.get(index).map(|(_, version)| version.as_str()));
            if let Some(expected) = expected {
                let row = row.ok_or_else(|| {
                    if write.constrained && write.id.is_some() {
                        ApiError::not_found("Resource is unavailable for this write")
                    } else {
                        ApiError::precondition_failed(
                            "Resource does not exist but If-Match was provided",
                        )
                    }
                })?;
                if row.version_id != expected {
                    if supplied.is_none() && patches.contains_key(index) {
                        return Err(
                            ApiError::conflict("Patch target changed after validation").into()
                        );
                    }
                    return Err(StorageError::version_conflict(expected, &row.version_id).into());
                }
            }
        }
        Ok(existed)
    }

    pub(crate) async fn authorize_results(
        &self,
        state: &AppState,
        tx: &dyn Transaction,
        entries: &[(usize, Value)],
        patches: &HashMap<usize, (Value, String)>,
        responses: &[Value],
        existed: &HashSet<usize>,
    ) -> Result<(), WriteError> {
        let mut rows = HashMap::new();
        let mut groups: BTreeMap<(&str, WriteMode), Vec<(usize, &PreparedWrite)>> = BTreeMap::new();
        for (index, write) in &self.writes {
            if write.mode == WriteMode::Delete
                || (write.mode == WriteMode::Create && existed.contains(index))
            {
                continue;
            }
            let resource = responses[*index]
                .get("resource")
                .ok_or_else(|| ApiError::internal("Missing transaction write result"))?;
            let id = resource["id"]
                .as_str()
                .ok_or_else(|| ApiError::internal("Missing transaction write identity"))?;
            let version = resource["meta"]["versionId"]
                .as_str()
                .ok_or_else(|| ApiError::internal("Missing transaction write version"))?;
            if resource["resourceType"].as_str() != Some(write.resource_type.as_str())
                || write.id.as_deref().is_some_and(|expected| id != expected)
            {
                return Err(ApiError::conflict("The stored write identity changed").into());
            }
            rows.insert(*index, (id, version, resource));
            groups
                .entry((&write.resource_type, write.mode))
                .or_default()
                .push((*index, write));
        }
        for group in groups.values() {
            let group_rows: Vec<_> = group
                .iter()
                .map(|(index, _)| (rows[index].0, rows[index].1))
                .collect();
            let visible = Self::visible_group(state, tx, group[0].1, &group_rows).await?;
            for (index, write) in group {
                let row = &rows[index];
                if visible.as_ref().is_some_and(|found| {
                    found
                        .get(row.0)
                        .is_none_or(|actual| actual.as_str() != row.1)
                }) {
                    return Err(
                        if write.mode == WriteMode::Update && !existed.contains(index) {
                            ApiError::not_found("Resource is unavailable for this write")
                        } else {
                            ApiError::forbidden("Proposed resource is outside the granted scope")
                        }
                        .into(),
                    );
                }
            }
        }
        for (index, entry) in entries {
            if let Some(row) = rows.get(index) {
                let candidate = patches
                    .get(index)
                    .map(|(body, _)| body)
                    .or_else(|| entry.get("resource"));
                self.writes[index]
                    .authorize_resource(state, row.0, row.2, candidate)
                    .await?;
            }
        }
        Ok(())
    }
}

/// Conditional predicates are shared only by entries with the same type and
/// write permission. A c predicate must never reuse the u/d result of a sibling.
#[derive(Default)]
pub(crate) struct PreparedTransactionConditions {
    params: BTreeMap<(String, WriteMode), SearchParams>,
    keys: HashMap<usize, (String, WriteMode)>,
}

impl PreparedTransactionConditions {
    pub(crate) async fn prepare(
        state: &AppState,
        entries: &[(usize, &Value)],
        writes: &PreparedTransactionWrites,
    ) -> Result<Self, ApiError> {
        let mut prepared = Self::default();
        for (index, entry) in entries {
            let Some(condition) = crate::handlers::entry_conditional_query(entry) else {
                continue;
            };
            let mode = match entry["request"]["method"]
                .as_str()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "POST" => WriteMode::Create,
                "PUT" | "PATCH" => WriteMode::Update,
                "DELETE" => WriteMode::Delete,
                _ => {
                    return Err(ApiError::bad_request(
                        "Unsupported conditional transaction write",
                    ));
                }
            };
            let key = (condition, mode);
            if !prepared.params.contains_key(&key) {
                let (resource_type, query) = key.0.split_once('?').expect("conditional query");
                let params = if let Some(write) = writes.get(*index) {
                    write.conditional(state, query).await?.params
                } else {
                    let cfg = state.search_config.config();
                    octofhir_db_postgres::queries::search::prepare_system_search_params(
                        &[resource_type],
                        &crate::handlers::conditional_search_params(query),
                        &cfg.registry,
                        state.terminology_provider.as_ref(),
                        state.config.search.max_valueset_expansion,
                    )
                    .await
                    .map_err(crate::handlers::map_storage_error)?
                };
                prepared.params.insert(key.clone(), params);
            }
            prepared.keys.insert(*index, key);
        }
        Ok(prepared)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(crate) fn params(&self, index: usize) -> Option<&SearchParams> {
        self.keys.get(&index).map(|key| &self.params[key])
    }

    /// Caller takes the exclusive gate before selection. Snapshot reads share
    /// bounded native SQL batches on the primary connection. Ambiguous bodies
    /// are locked by type for policies; single bodies use authorize_previous.
    pub(crate) async fn select(
        &self,
        state: &AppState,
        tx: &dyn Transaction,
        writes: &PreparedTransactionWrites,
        entries: &[(usize, &Value)],
        proposed: &[octofhir_storage::ProposedResource],
    ) -> Result<HashMap<usize, Arc<Vec<StoredResource>>>, WriteError> {
        let keys: std::collections::BTreeSet<_> = entries
            .iter()
            .filter_map(|(index, _)| self.keys.get(index))
            .collect();
        let predicates: Vec<_> = keys
            .iter()
            .map(|key| {
                (
                    key.0
                        .split_once('?')
                        .expect("conditional query")
                        .0
                        .to_owned(),
                    self.params[*key].clone(),
                )
            })
            .collect();
        let selections = if proposed.is_empty() {
            tx.search_conditional_batch(&predicates).await?
        } else {
            tx.search_proposed_conditional_batch(&predicates, proposed)
                .await?
        };
        if selections.len() != keys.len() {
            return Err(ApiError::internal("Incomplete conditional search results").into());
        }
        let selected: BTreeMap<_, _> = keys.into_iter().zip(selections).collect();
        let mut ids_by_type: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        // Ambiguous matches need locked policy checks before a 412 response.
        // Single matches are locked once by authorize_previous in the final
        // transaction; its gate pins this snapshot until that grouped lock.
        for (key, rows) in &selected {
            if rows.len() > 1 {
                let resource_type = key.0.split_once('?').expect("conditional query").0;
                ids_by_type
                    .entry(resource_type)
                    .or_default()
                    .extend(rows.iter().map(|row| row.id.clone()));
            }
        }
        let mut locked = HashMap::new();
        for (resource_type, mut ids) in ids_by_type {
            ids.sort();
            ids.dedup();
            if !ids.is_empty() {
                for row in tx.read_many_for_update(resource_type, &ids).await? {
                    locked.insert((resource_type, row.id.clone()), row);
                }
            }
        }
        let mut shared = BTreeMap::new();
        for (key, mut rows) in selected {
            let resource_type = key.0.split_once('?').expect("conditional query").0;
            if rows.len() > 1 {
                for row in &mut rows {
                    let actual = locked
                        .get(&(resource_type, row.id.clone()))
                        .ok_or_else(|| {
                            ApiError::conflict("Conditional target changed during selection")
                        })?;
                    if row.version_id != actual.version_id || row.resource != actual.resource {
                        return Err(ApiError::conflict(
                            "Conditional target changed during selection",
                        )
                        .into());
                    }
                    *row = actual.clone();
                }
            }
            shared.insert(key, Arc::new(rows));
        }
        let mut matches = HashMap::new();
        for (index, entry) in entries {
            let Some(key) = self.keys.get(index) else {
                continue;
            };
            // Duplicate criteria share bodies, not just SQL. Large existing
            // matches must not be cloned once per ignored POST payload.
            let rows = shared[key].clone();
            // Check policies before disclosing an ambiguous selection. A single
            // result is authorized with its resolved candidate by the ordinary
            // locked group protocol before any mutation or response.
            if rows.len() > 1
                && !entry["request"]["method"]
                    .as_str()
                    .is_some_and(|m| m.eq_ignore_ascii_case("PATCH"))
                && let Some(write) = writes.get(*index)
            {
                for row in rows.iter() {
                    let mut candidate = entry.get("resource").cloned();
                    if write.mode == WriteMode::Create {
                        candidate = Some(row.resource.clone());
                    } else if let Some(candidate) = &mut candidate
                        && candidate["id"].as_str().is_none()
                    {
                        candidate["id"] = Value::String(row.id.clone());
                    }
                    write
                        .resolved(Some(&row.id))
                        .authorize_snapshot(state, row, candidate.as_ref())
                        .await?;
                }
            }
            matches.insert(*index, rows);
        }
        Ok(matches)
    }
}

pub(crate) struct PreparedConditionalWrite<'a> {
    write: &'a PreparedWrite,
    params: SearchParams,
}

impl PreparedConditionalWrite<'_> {
    async fn select_rows(
        &self,
        state: &AppState,
        tx: &mut dyn Transaction,
    ) -> Result<Vec<StoredResource>, WriteError> {
        tx.lock_conditional_writes().await?;
        let matches = tx
            .search_system_raw(
                &[self.write.resource_type.as_str()],
                &self.params,
                state.config.search.include_limits,
                true,
            )
            .await?;
        let mut selected = Vec::new();
        for row in &matches.entries {
            let previous = tx
                .read_for_update(&self.write.resource_type, &row.id)
                .await?
                .ok_or_else(|| ApiError::not_found("Resource is unavailable for this write"))?;
            if !self.write.visible(state, tx, &previous).await? {
                return Err(ApiError::not_found("Resource is unavailable for this write").into());
            }
            selected.push(previous);
        }
        Ok(selected)
    }

    async fn select(
        &self,
        state: &AppState,
        tx: &mut dyn Transaction,
        candidate: Option<&Value>,
    ) -> Result<Option<StoredResource>, WriteError> {
        let mut selected = self.select_rows(state, tx).await?;
        for previous in &selected {
            let resolved = self.write.resolved(Some(&previous.id));
            let mut effective = candidate.cloned();
            if self.write.mode == WriteMode::Create {
                effective = Some(previous.resource.clone());
            } else if let Some(candidate) = &mut effective
                && candidate["id"].as_str().is_none()
            {
                candidate["id"] = Value::String(previous.id.clone());
            }
            resolved
                .authorize_snapshot(state, previous, effective.as_ref())
                .await?;
        }
        if selected.len() > 1 {
            return Err(ApiError::precondition_failed(
                "Multiple resources match the conditional criteria",
            )
            .into());
        }
        Ok(selected.pop())
    }

    /// PATCH must construct the proposed resource before evaluating policies
    /// that depend on request.body. Its caller authorizes each preview before
    /// returning cardinality or patch errors, just like a direct PATCH.
    pub(crate) async fn patch_snapshots(
        &self,
        state: &AppState,
    ) -> Result<Vec<StoredResource>, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = self.select_rows(state, &mut *tx).await;
        tx.rollback().await?;
        result
    }

    /// Release preview locks before CPU/schema work. Every mutation resolves
    /// the condition again under the exclusive gate before applying its write.
    pub(crate) async fn snapshot(
        &self,
        state: &AppState,
        candidate: Option<&Value>,
    ) -> Result<Option<StoredResource>, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = self.select(state, &mut *tx, candidate).await;
        tx.rollback().await?;
        result
    }

    /// None means an omitted body ID now resolves to a different target. The
    /// caller must obtain that identity and validate its proposed self-links
    /// again outside the transaction instead of writing an unchecked rebasing.
    pub(crate) async fn update(
        &self,
        state: &AppState,
        candidate: &Value,
        references: &LocalReferenceSet,
        supplied_id: Option<&str>,
        preview_id: Option<&str>,
        if_match: Option<&str>,
    ) -> Result<Option<WriteResult>, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = async {
            let mut selection_body = candidate.clone();
            if supplied_id.is_none() {
                selection_body
                    .as_object_mut()
                    .expect("prepared resource")
                    .remove("id");
            }
            let current = self.select(state, &mut *tx, Some(&selection_body)).await?;
            let current_id = current.as_ref().map(|row| row.id.as_str());
            if let (Some(supplied), Some(current)) = (supplied_id, current_id)
                && supplied != current
            {
                return Err(ApiError::bad_request(
                    "Conditional PUT body id does not match the selected resource",
                )
                .into());
            }
            if supplied_id.is_none() && current_id != preview_id {
                return Ok(None);
            }
            let resolved = self.write.resolved(current_id.or(supplied_id));
            resolved
                .apply(state, &mut *tx, Some(candidate), references, if_match)
                .await
                .map(Some)
        }
        .await;
        if matches!(result, Ok(Some(_))) {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }

    pub(crate) async fn patch(
        &self,
        state: &AppState,
        previous: &StoredResource,
        candidate: &Value,
        references: &LocalReferenceSet,
        if_match: Option<&str>,
    ) -> Result<WriteResult, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = async {
            let current = self
                .select(state, &mut *tx, Some(candidate))
                .await?
                .ok_or_else(|| {
                    ApiError::not_found("No resource matches the conditional criteria")
                })?;
            if current.id != previous.id || current.version_id != previous.version_id {
                return Err(ApiError::precondition_failed(
                    "Conditional target changed while the patch was prepared",
                )
                .into());
            }
            self.write
                .resolved(Some(&current.id))
                .apply(
                    state,
                    &mut *tx,
                    Some(candidate),
                    references,
                    if_match.or(Some(&previous.version_id)),
                )
                .await
        }
        .await;
        if result.is_ok() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }

    pub(crate) async fn delete(
        &self,
        state: &AppState,
        if_match: Option<&str>,
    ) -> Result<WriteResult, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = async {
            let current = self.select(state, &mut *tx, None).await?;
            let mut resolved = self
                .write
                .resolved(current.as_ref().map(|row| row.id.as_str()));
            if current.is_none() {
                // An empty conditional selection is idempotent even for a
                // filtered grant; it does not identify a hidden instance.
                resolved.constrained = false;
            }
            resolved
                .apply(
                    state,
                    &mut *tx,
                    None,
                    &LocalReferenceSet::default(),
                    if_match,
                )
                .await
        }
        .await;
        if result.is_ok() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }

    /// A matching POST is ignored, including its content-validation failures.
    /// Prepare content outside the pool transaction only when no match exists;
    /// execute searches again before committing the actual create.
    pub(crate) async fn execute(
        &self,
        state: &AppState,
        candidate: &Value,
        references: Option<&LocalReferenceSet>,
    ) -> Result<Option<WriteResult>, WriteError> {
        let mut tx = state.storage.begin_transaction().await?;
        let result = async {
            let selected = self.select(state, &mut *tx, Some(candidate)).await?;
            if let Some(previous) = selected {
                return Ok(Some(WriteResult {
                    deletion: None,
                    stored: Some(previous.clone()),
                    previous: Some(previous),
                    created: false,
                }));
            }
            match references {
                Some(references) => self
                    .write
                    .apply(state, &mut *tx, Some(candidate), references, None)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        }
        .await;
        if result.is_ok() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }
}
