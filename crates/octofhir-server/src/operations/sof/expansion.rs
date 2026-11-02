//! A ValueSet's membership, computed strictly for a query relation.
//!
//! A terminology dependency of an SQLQuery must resolve to its whole
//! membership or fail the request (SQL on FHIR, "Terminology in SQL": "one
//! resolution per job"): an expansion that silently leaves out an import it
//! could not resolve, or a filter it does not evaluate, would return wrong
//! rows rather than a rejected request. So, unlike a best-effort `$expand`,
//! this expander says why it cannot determine a membership whenever any part
//! of the definition is beyond it:
//!
//! - a ValueSet that carries an expansion is taken as that expansion
//!   (rejected if it is a page or otherwise incomplete);
//! - `compose.include` / `exclude` by enumerated concepts, by a whole code
//!   system, by filters and by imported value sets (intersected with the
//!   system part, as FHIR defines);
//! - a code system must be found with `content = complete`; filters work on
//!   its hierarchy (`concept`: `is-a`, `descendent-of`, `is-not-a`,
//!   `generalizes`, `child-of`, `descendent-leaf`, `=`, `in`, `not-in`,
//!   `regex`) and on its declared properties (`=`, `in`, `not-in`, `regex`,
//!   `exists`);
//! - `compose.inactive = false` drops inactive codes; otherwise they stay,
//!   flagged.
//!
//! The result is a `ValueSet.expansion` whose parameters record the code
//! system versions (`version`, `system|version`) and imported value sets
//! (`used-valueset`) the membership was computed from.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};

use super::terminology::{MAX_RELATION_ROWS, incomplete_expansion};
use crate::operations::OperationError;

/// Value sets importing value sets: deeper than this is refused.
const MAX_IMPORT_DEPTH: usize = 16;

/// Longest regular expression a filter may use.
const MAX_REGEX_LENGTH: usize = 1_000;

/// A canonical resource found by a [`TerminologySource`], and where from.
#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    pub resource: Value,
    /// Where the resource came from, for provenance (`context`,
    /// `ValueSet/123`, `package hl7.fhir.r4.core#4.0.1`, ...).
    pub origin: String,
}

/// The outcome of looking a canonical up.
#[derive(Clone, Debug)]
pub(crate) enum Lookup {
    Found(Resolved),
    Missing,
    /// No version was given and more than one is available.
    Ambiguous(String),
}

/// Where value sets and code systems are looked up.
#[async_trait]
pub(crate) trait TerminologySource: Send + Sync {
    async fn find(
        &self,
        resource_type: &str,
        url: &str,
        version: Option<&str>,
    ) -> Result<Lookup, OperationError>;
}

/// A member of a value set.
#[derive(Clone, Debug)]
struct Member {
    system: String,
    version: Option<String>,
    code: String,
    display: Option<String>,
    inactive: bool,
    is_abstract: bool,
}

impl Member {
    fn key(&self) -> (String, String) {
        (self.system.clone(), self.code.clone())
    }
}

/// Members in first-seen order, unique by (system, code).
#[derive(Default)]
struct Members {
    list: Vec<Member>,
    keys: HashSet<(String, String)>,
}

impl Members {
    fn add(&mut self, member: Member) -> Result<(), OperationError> {
        if self.keys.insert(member.key()) {
            if self.list.len() >= MAX_RELATION_ROWS {
                return Err(undeterminable(format!(
                    "it has more than {MAX_RELATION_ROWS} members, more than a query relation may hold"
                )));
            }
            self.list.push(member);
        }
        Ok(())
    }

    fn retain(&mut self, keep: impl Fn(&Member) -> bool) {
        self.list.retain(|member| keep(member));
        self.keys = self.list.iter().map(Member::key).collect();
    }
}

/// One concept of a code system, with its place in the hierarchy.
struct Concept {
    code: String,
    display: Option<String>,
    inactive: bool,
    is_abstract: bool,
    parents: Vec<usize>,
    children: Vec<usize>,
    /// (property code, value as text)
    properties: Vec<(String, String)>,
}

/// A code system whose concepts can be enumerated.
struct CodeSystemIndex {
    url: String,
    version: Option<String>,
    concepts: Vec<Concept>,
    by_code: HashMap<String, usize>,
    /// Codes of the properties the code system declares.
    declared: HashSet<String>,
}

/// Computes memberships against one [`TerminologySource`]; code systems are
/// read once per expander.
pub(crate) struct Expander<'a> {
    source: &'a dyn TerminologySource,
    code_systems: HashMap<(String, Option<String>), Arc<CodeSystemIndex>>,
    used_code_systems: BTreeSet<String>,
    used_value_sets: BTreeSet<String>,
    /// Canonicals of the value sets being expanded, to refuse a cycle.
    path: Vec<String>,
}

type BoxFuture<'s, T> = Pin<Box<dyn Future<Output = T> + Send + 's>>;

impl<'a> Expander<'a> {
    pub(crate) fn new(source: &'a dyn TerminologySource) -> Self {
        Self {
            source,
            code_systems: HashMap::new(),
            used_code_systems: BTreeSet::new(),
            used_value_sets: BTreeSet::new(),
            path: Vec::new(),
        }
    }

    /// The expansion of `value_set`: its own `expansion` when it carries one,
    /// else one computed from `compose`. Errors are 422s naming what could
    /// not be determined.
    pub(crate) async fn expand(mut self, value_set: &Value) -> Result<Value, OperationError> {
        if let Some(expansion) = value_set.get("expansion") {
            if let Some(reason) = incomplete_expansion(expansion) {
                return Err(undeterminable(reason));
            }
            return Ok(expansion.clone());
        }
        if let Some(url) = value_set["url"].as_str() {
            self.path.push(url.to_owned());
        }
        let members = self.members_of(value_set).await?;
        let mut parameters: Vec<Value> = self
            .used_code_systems
            .iter()
            .map(|system| json!({"name": "version", "valueUri": system}))
            .collect();
        parameters.extend(
            self.used_value_sets
                .iter()
                .map(|value_set| json!({"name": "used-valueset", "valueUri": value_set})),
        );
        let contains: Vec<Value> = members
            .list
            .iter()
            .map(|member| {
                let mut entry = json!({"system": member.system, "code": member.code});
                if let Some(version) = &member.version {
                    entry["version"] = json!(version);
                }
                if let Some(display) = &member.display {
                    entry["display"] = json!(display);
                }
                if member.inactive {
                    entry["inactive"] = json!(true);
                }
                if member.is_abstract {
                    entry["abstract"] = json!(true);
                }
                entry
            })
            .collect();
        let mut expansion = json!({
            "identifier": format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            "timestamp": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "total": contains.len(),
            "contains": contains,
        });
        if !parameters.is_empty() {
            expansion["parameter"] = Value::Array(parameters);
        }
        Ok(expansion)
    }

    /// Members of a ValueSet: from its expansion, else its compose.
    fn members_of<'s>(
        &'s mut self,
        value_set: &'s Value,
    ) -> BoxFuture<'s, Result<Members, OperationError>> {
        Box::pin(async move {
            if let Some(expansion) = value_set.get("expansion") {
                return expansion_members(expansion);
            }
            let compose = value_set.get("compose").ok_or_else(|| {
                undeterminable(format!(
                    "ValueSet {} has neither an expansion nor a compose",
                    describe(value_set)
                ))
            })?;
            let mut members = Members::default();
            for include in compose["include"].as_array().into_iter().flatten() {
                for member in self.include(include).await?.list {
                    members.add(member)?;
                }
            }
            let mut excluded: HashSet<(String, String)> = HashSet::new();
            for exclude in compose["exclude"].as_array().into_iter().flatten() {
                excluded.extend(self.include(exclude).await?.keys);
            }
            if !excluded.is_empty() {
                members.retain(|member| !excluded.contains(&member.key()));
            }
            if compose["inactive"] == false {
                members.retain(|member| !member.inactive);
            }
            Ok(members)
        })
    }

    /// The codes one `include` (or `exclude`) selects: its system part
    /// intersected with every value set it imports.
    async fn include(&mut self, include: &Value) -> Result<Members, OperationError> {
        let system = include["system"].as_str();
        let version = include["version"].as_str();
        let mut members = match system {
            Some(system) => Some(self.system_part(system, version, include).await?),
            None => None,
        };
        for canonical in include["valueSet"].as_array().into_iter().flatten() {
            let canonical = canonical
                .as_str()
                .ok_or_else(|| undeterminable("compose.include.valueSet is not a canonical"))?;
            let imported = self.import(canonical).await?;
            members = Some(match members {
                None => imported,
                Some(mut members) => {
                    members.retain(|member| imported.keys.contains(&member.key()));
                    members
                }
            });
        }
        members
            .ok_or_else(|| undeterminable("a compose.include has neither a system nor a valueSet"))
    }

    /// The codes of `system` an include selects: enumerated, filtered or all.
    async fn system_part(
        &mut self,
        system: &str,
        version: Option<&str>,
        include: &Value,
    ) -> Result<Members, OperationError> {
        let mut members = Members::default();
        if let Some(concepts) = include["concept"].as_array() {
            // Enumerated codes are members as listed; the code system, when
            // available, checks them and supplies version and status.
            let index = self.optional_code_system(system, version).await?;
            for concept in concepts {
                let code = concept["code"]
                    .as_str()
                    .ok_or_else(|| undeterminable(format!("a concept of {system} has no code")))?;
                let known = match &index {
                    Some(index) => Some(
                        index
                            .by_code
                            .get(code)
                            .map(|&i| &index.concepts[i])
                            .ok_or_else(|| {
                                undeterminable(format!(
                                    "code {code} is not in code system {}",
                                    cs_label(index)
                                ))
                            })?,
                    ),
                    None => None,
                };
                members.add(Member {
                    system: system.to_owned(),
                    version: index
                        .as_ref()
                        .and_then(|index| index.version.clone())
                        .or_else(|| version.map(str::to_owned)),
                    code: code.to_owned(),
                    display: concept["display"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| known.and_then(|known| known.display.clone())),
                    inactive: known.is_some_and(|known| known.inactive),
                    is_abstract: known.is_some_and(|known| known.is_abstract),
                })?;
            }
            return Ok(members);
        }
        let index = self.code_system(system, version).await?;
        let selected: Vec<usize> = match include["filter"].as_array() {
            Some(filters) if !filters.is_empty() => {
                let mut selected: Option<HashSet<usize>> = None;
                for filter in filters {
                    let matched = apply_filter(&index, filter)?;
                    selected = Some(match selected {
                        None => matched,
                        Some(previous) => previous.intersection(&matched).copied().collect(),
                    });
                }
                let selected = selected.unwrap_or_default();
                (0..index.concepts.len())
                    .filter(|i| selected.contains(i))
                    .collect()
            }
            _ => (0..index.concepts.len()).collect(),
        };
        for i in selected {
            let concept = &index.concepts[i];
            members.add(Member {
                system: index.url.clone(),
                version: index.version.clone(),
                code: concept.code.clone(),
                display: concept.display.clone(),
                inactive: concept.inactive,
                is_abstract: concept.is_abstract,
            })?;
        }
        Ok(members)
    }

    /// The members of an imported value set.
    fn import<'s>(
        &'s mut self,
        canonical: &'s str,
    ) -> BoxFuture<'s, Result<Members, OperationError>> {
        Box::pin(async move {
            if self.path.len() >= MAX_IMPORT_DEPTH {
                return Err(undeterminable(format!(
                    "value sets import value sets deeper than {MAX_IMPORT_DEPTH} levels"
                )));
            }
            let (url, version) = split_canonical(canonical);
            if self.path.iter().any(|seen| seen == url) {
                return Err(undeterminable(format!("ValueSet {url} imports itself")));
            }
            let resolved = match self.source.find("ValueSet", url, version).await? {
                Lookup::Found(resolved) => resolved,
                Lookup::Missing => {
                    return Err(undeterminable(format!(
                        "it imports ValueSet {canonical}, which cannot be resolved"
                    )));
                }
                Lookup::Ambiguous(reason) => {
                    return Err(undeterminable(format!("it imports {reason}")));
                }
            };
            let value_set = resolved.resource;
            self.used_value_sets
                .insert(match value_set["version"].as_str() {
                    Some(version) => format!("{url}|{version}"),
                    None => url.to_owned(),
                });
            self.path.push(url.to_owned());
            let members = self.members_of(&value_set).await;
            self.path.pop();
            members
        })
    }

    /// A code system the include needs to enumerate: found, complete.
    async fn code_system(
        &mut self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Arc<CodeSystemIndex>, OperationError> {
        match self.lookup_code_system(url, version).await? {
            Ok(index) => Ok(index),
            Err(reason) => Err(undeterminable(reason)),
        }
    }

    /// A code system that checks enumerated codes when it can be found; one
    /// that cannot be enumerated here does not check them.
    async fn optional_code_system(
        &mut self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Arc<CodeSystemIndex>>, OperationError> {
        Ok(self.lookup_code_system(url, version).await?.ok())
    }

    async fn lookup_code_system(
        &mut self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Result<Arc<CodeSystemIndex>, String>, OperationError> {
        let key = (url.to_owned(), version.map(str::to_owned));
        if let Some(index) = self.code_systems.get(&key) {
            return Ok(Ok(index.clone()));
        }
        let named = match version {
            Some(version) => format!("{url}|{version}"),
            None => url.to_owned(),
        };
        let resolved = match self.source.find("CodeSystem", url, version).await? {
            Lookup::Found(resolved) => resolved,
            Lookup::Missing => {
                return Ok(Err(format!(
                    "code system {named} cannot be resolved on this server"
                )));
            }
            Lookup::Ambiguous(reason) => return Ok(Err(reason)),
        };
        let code_system = &resolved.resource;
        match code_system["content"].as_str() {
            None | Some("complete") => {}
            Some(content) => {
                return Ok(Err(format!(
                    "code system {named} has content = {content}, so its codes cannot be enumerated here"
                )));
            }
        }
        let index = Arc::new(index_code_system(url, code_system)?);
        self.used_code_systems.insert(match &index.version {
            Some(version) => format!("{url}|{version}"),
            None => url.to_owned(),
        });
        self.code_systems.insert(key, index.clone());
        Ok(Ok(index))
    }
}

/// Members listed by an expansion (any depth, abstract entries flagged).
fn expansion_members(expansion: &Value) -> Result<Members, OperationError> {
    if let Some(reason) = incomplete_expansion(expansion) {
        return Err(undeterminable(reason));
    }
    let mut members = Members::default();
    let mut pending: Vec<&Value> = vec![&expansion["contains"]];
    while let Some(list) = pending.pop() {
        for entry in list.as_array().into_iter().flatten() {
            pending.push(&entry["contains"]);
            let Some(code) = entry["code"].as_str() else {
                continue;
            };
            let system = entry["system"].as_str().ok_or_else(|| {
                undeterminable(format!("an expansion lists code {code} without a system"))
            })?;
            members.add(Member {
                system: system.to_owned(),
                version: entry["version"].as_str().map(str::to_owned),
                code: code.to_owned(),
                display: entry["display"].as_str().map(str::to_owned),
                inactive: entry["inactive"] == true,
                is_abstract: entry["abstract"] == true,
            })?;
        }
    }
    Ok(members)
}

/// Standard concept properties, by the URI a code system declares them with.
const PARENT: &str = "http://hl7.org/fhir/concept-properties#parent";
const INACTIVE: &str = "http://hl7.org/fhir/concept-properties#inactive";
const STATUS: &str = "http://hl7.org/fhir/concept-properties#status";
const NOT_SELECTABLE: &str = "http://hl7.org/fhir/concept-properties#notSelectable";

fn index_code_system(url: &str, code_system: &Value) -> Result<CodeSystemIndex, OperationError> {
    // Local property codes of the standard properties: declared by URI, or
    // used under their standard code.
    let declared: HashMap<String, Option<String>> = code_system["property"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|property| {
            Some((
                property["code"].as_str()?.to_owned(),
                property["uri"].as_str().map(str::to_owned),
            ))
        })
        .collect();
    let local = |uri: &str, standard: &str| -> String {
        declared
            .iter()
            .find(|(_, declared_uri)| declared_uri.as_deref() == Some(uri))
            .map(|(code, _)| code.clone())
            .unwrap_or_else(|| standard.to_owned())
    };
    let parent_code = local(PARENT, "parent");
    let inactive_code = local(INACTIVE, "inactive");
    let status_code = local(STATUS, "status");
    let not_selectable_code = local(NOT_SELECTABLE, "notSelectable");

    let mut concepts: Vec<Concept> = Vec::new();
    let mut by_code: HashMap<String, usize> = HashMap::new();
    let mut parent_codes: Vec<(usize, String)> = Vec::new();
    // (concept, index of the concept it nests under)
    let mut pending: Vec<(&Value, Option<usize>)> = code_system["concept"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .map(|concept| (concept, None))
        .collect();
    while let Some((concept, nested_under)) = pending.pop() {
        let code = concept["code"]
            .as_str()
            .ok_or_else(|| undeterminable(format!("a concept of code system {url} has no code")))?;
        let properties: Vec<(String, String)> = concept["property"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|property| {
                Some((
                    property["code"].as_str()?.to_owned(),
                    property_text(property)?,
                ))
            })
            .collect();
        let has = |name: &str, value: &str| {
            properties
                .iter()
                .any(|(code, text)| code == name && text == value)
        };
        let index = concepts.len();
        if by_code.insert(code.to_owned(), index).is_some() {
            return Err(undeterminable(format!(
                "code system {url} defines code {code} twice"
            )));
        }
        for (property, value) in &properties {
            if *property == parent_code {
                parent_codes.push((index, value.clone()));
            }
        }
        concepts.push(Concept {
            code: code.to_owned(),
            display: concept["display"].as_str().map(str::to_owned),
            inactive: has(&inactive_code, "true")
                || has(&status_code, "retired")
                || has(&status_code, "inactive"),
            is_abstract: has(&not_selectable_code, "true"),
            parents: nested_under.into_iter().collect(),
            children: Vec::new(),
            properties,
        });
        if concepts.len() > MAX_RELATION_ROWS {
            return Err(undeterminable(format!(
                "code system {url} has more than {MAX_RELATION_ROWS} concepts"
            )));
        }
        for child in concept["concept"].as_array().into_iter().flatten().rev() {
            pending.push((child, Some(index)));
        }
    }
    for (child, parent_code) in parent_codes {
        if let Some(&parent) = by_code.get(&parent_code)
            && !concepts[child].parents.contains(&parent)
        {
            concepts[child].parents.push(parent);
        }
    }
    for child in 0..concepts.len() {
        for parent in concepts[child].parents.clone() {
            concepts[parent].children.push(child);
        }
    }
    Ok(CodeSystemIndex {
        url: url.to_owned(),
        version: code_system["version"].as_str().map(str::to_owned),
        concepts,
        by_code,
        declared: declared.into_keys().collect(),
    })
}

/// A concept property's value as text (a Coding by its code).
fn property_text(property: &Value) -> Option<String> {
    let object = property.as_object()?;
    for (key, value) in object {
        if !key.starts_with("value") {
            continue;
        }
        return match value {
            Value::String(text) => Some(text.clone()),
            Value::Bool(flag) => Some(flag.to_string()),
            Value::Number(number) => Some(number.to_string()),
            Value::Object(coding) => coding.get("code")?.as_str().map(str::to_owned),
            _ => None,
        };
    }
    None
}

/// The concepts one `compose.include.filter` selects.
fn apply_filter(index: &CodeSystemIndex, filter: &Value) -> Result<HashSet<usize>, OperationError> {
    let property = filter["property"].as_str().unwrap_or_default();
    let op = filter["op"].as_str().unwrap_or_default();
    let value = filter["value"].as_str().unwrap_or_default();
    let all = || (0..index.concepts.len()).collect::<HashSet<usize>>();
    let codes = |list: &str| -> HashSet<usize> {
        list.split(',')
            .filter_map(|code| index.by_code.get(code.trim()).copied())
            .collect()
    };
    let unsupported = || {
        undeterminable(format!(
            "filter {property} {op} {value} on code system {} is not evaluated here",
            cs_label(index)
        ))
    };
    if property == "concept" || (property == "code" && !index.declared.contains("code")) {
        let code = index.by_code.get(value).copied();
        return Ok(match op {
            "is-a" => code.map(|i| with_descendants(index, i)).unwrap_or_default(),
            "descendent-of" => code
                .map(|i| {
                    let mut set = with_descendants(index, i);
                    set.remove(&i);
                    set
                })
                .unwrap_or_default(),
            "is-not-a" => {
                let excluded = code.map(|i| with_descendants(index, i)).unwrap_or_default();
                all().difference(&excluded).copied().collect()
            }
            "generalizes" => code.map(|i| with_ancestors(index, i)).unwrap_or_default(),
            "child-of" => code
                .map(|i| index.concepts[i].children.iter().copied().collect())
                .unwrap_or_default(),
            "descendent-leaf" => code
                .map(|i| {
                    with_descendants(index, i)
                        .into_iter()
                        .filter(|&d| d != i && index.concepts[d].children.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            "=" => code.into_iter().collect(),
            "in" => codes(value),
            "not-in" => all().difference(&codes(value)).copied().collect(),
            "regex" => {
                let regex = filter_regex(value)?;
                all()
                    .into_iter()
                    .filter(|&i| regex.is_match(&index.concepts[i].code))
                    .collect()
            }
            _ => return Err(unsupported()),
        });
    }
    if !index.declared.contains(property) {
        return Err(undeterminable(format!(
            "a filter uses property {property}, which code system {} does not declare",
            cs_label(index)
        )));
    }
    let values_of = |i: usize| {
        index.concepts[i]
            .properties
            .iter()
            .filter(move |(code, _)| code == property)
            .map(|(_, text)| text.as_str())
    };
    let select = |keep: &dyn Fn(usize) -> bool| -> HashSet<usize> {
        all().into_iter().filter(|&i| keep(i)).collect()
    };
    Ok(match op {
        "=" => select(&|i| values_of(i).any(|text| text == value)),
        "in" => {
            let wanted: HashSet<&str> = value.split(',').map(str::trim).collect();
            select(&|i| values_of(i).any(|text| wanted.contains(text)))
        }
        "not-in" => {
            let unwanted: HashSet<&str> = value.split(',').map(str::trim).collect();
            select(&|i| !values_of(i).any(|text| unwanted.contains(text)))
        }
        "regex" => {
            let regex = filter_regex(value)?;
            select(&|i| values_of(i).any(|text| regex.is_match(text)))
        }
        "exists" => match value {
            "true" => select(&|i| values_of(i).next().is_some()),
            "false" => select(&|i| values_of(i).next().is_none()),
            _ => return Err(unsupported()),
        },
        _ => return Err(unsupported()),
    })
}

/// A filter's regular expression, matched against the whole value.
fn filter_regex(pattern: &str) -> Result<Regex, OperationError> {
    if pattern.len() > MAX_REGEX_LENGTH {
        return Err(undeterminable(format!(
            "a regex filter is longer than {MAX_REGEX_LENGTH} characters"
        )));
    }
    RegexBuilder::new(&format!("^(?:{pattern})$"))
        .size_limit(1 << 20)
        .build()
        .map_err(|error| undeterminable(format!("invalid regex filter {pattern}: {error}")))
}

fn with_descendants(index: &CodeSystemIndex, root: usize) -> HashSet<usize> {
    walk(root, |i| &index.concepts[i].children)
}

fn with_ancestors(index: &CodeSystemIndex, root: usize) -> HashSet<usize> {
    walk(root, |i| &index.concepts[i].parents)
}

fn walk<'i>(root: usize, next: impl Fn(usize) -> &'i Vec<usize>) -> HashSet<usize> {
    let mut seen = HashSet::from([root]);
    let mut pending = vec![root];
    while let Some(i) = pending.pop() {
        for &j in next(i) {
            if seen.insert(j) {
                pending.push(j);
            }
        }
    }
    seen
}

fn cs_label(index: &CodeSystemIndex) -> String {
    match &index.version {
        Some(version) => format!("{}|{version}", index.url),
        None => index.url.clone(),
    }
}

fn describe(value_set: &Value) -> String {
    match (value_set["url"].as_str(), value_set["version"].as_str()) {
        (Some(url), Some(version)) => format!("{url}|{version}"),
        (Some(url), None) => url.to_owned(),
        _ => "(without url)".to_owned(),
    }
}

/// `url|version` → (url, version).
pub(crate) fn split_canonical(canonical: &str) -> (&str, Option<&str>) {
    match canonical.split_once('|') {
        Some((url, version)) if !version.is_empty() => (url, Some(version)),
        Some((url, _)) => (url, None),
        None => (canonical, None),
    }
}

/// A 422 for a value set whose membership cannot be determined.
pub(crate) fn undeterminable(reason: impl std::fmt::Display) -> OperationError {
    OperationError::Api(octofhir_api::ApiError::unprocessable_entity(
        format!("membership cannot be determined: {reason}"),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::sof::terminology::value_set_rows;

    /// Artifacts by (type, url), each with a version.
    struct Fixtures(Vec<Value>);

    #[async_trait]
    impl TerminologySource for Fixtures {
        async fn find(
            &self,
            resource_type: &str,
            url: &str,
            version: Option<&str>,
        ) -> Result<Lookup, OperationError> {
            let found: Vec<&Value> = self
                .0
                .iter()
                .filter(|resource| {
                    resource["resourceType"] == resource_type
                        && resource["url"] == url
                        && version.is_none_or(|version| resource["version"] == version)
                })
                .collect();
            Ok(match found.as_slice() {
                [] => Lookup::Missing,
                [one] => Lookup::Found(Resolved {
                    resource: (*one).clone(),
                    origin: "fixture".into(),
                }),
                _ => Lookup::Ambiguous(format!("{url} has several versions")),
            })
        }
    }

    const CS: &str = "http://example.org/cs";

    fn code_system() -> Value {
        json!({
            "resourceType": "CodeSystem", "url": CS, "version": "3", "content": "complete",
            "property": [
                {"code": "status", "uri": STATUS, "type": "code"},
                {"code": "abstract", "uri": NOT_SELECTABLE, "type": "boolean"},
                {"code": "colour", "type": "string"}
            ],
            "concept": [
                {"code": "disorder", "display": "Disorder",
                 "property": [{"code": "abstract", "valueBoolean": true}],
                 "concept": [
                    {"code": "heart", "display": "Heart disease",
                     "property": [{"code": "colour", "valueString": "red"}],
                     "concept": [
                        {"code": "mi", "display": "Myocardial infarction"},
                        {"code": "old-mi", "display": "Old MI",
                         "property": [{"code": "status", "valueCode": "retired"}]}
                     ]},
                    {"code": "diabetes", "display": "Diabetes",
                     "property": [{"code": "colour", "valueString": "blue"}]}
                 ]},
                {"code": "well", "display": "Fit and well"}
            ]
        })
    }

    fn value_set(compose: Value) -> Value {
        json!({"resourceType": "ValueSet", "url": "http://example.org/vs", "compose": compose})
    }

    async fn codes(fixtures: Vec<Value>, value_set: &Value) -> Result<Vec<String>, OperationError> {
        let source = Fixtures(fixtures);
        let expansion = Expander::new(&source).expand(value_set).await?;
        Ok(value_set_rows(&expansion)
            .unwrap()
            .iter()
            .map(|row| row["code"].as_str().unwrap().to_owned())
            .collect())
    }

    fn message(error: OperationError) -> String {
        match error {
            OperationError::Api(api) => api.to_string(),
            other => format!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn whole_systems_hierarchy_filters_and_excludes() {
        // Abstract concepts are not members; inactive ones are, flagged.
        let all = codes(
            vec![code_system()],
            &value_set(json!({"include": [{"system": CS}]})),
        )
        .await
        .unwrap();
        assert_eq!(all, ["heart", "mi", "old-mi", "diabetes", "well"]);

        let heart = codes(
            vec![code_system()],
            &value_set(json!({"include": [{"system": CS,
                "filter": [{"property": "concept", "op": "is-a", "value": "heart"}]}],
                "exclude": [{"system": CS, "concept": [{"code": "old-mi"}]}]})),
        )
        .await
        .unwrap();
        assert_eq!(heart, ["heart", "mi"]);

        let below = codes(
            vec![code_system()],
            &value_set(json!({"include": [{"system": CS, "filter": [
                {"property": "concept", "op": "descendent-of", "value": "disorder"},
                {"property": "colour", "op": "in", "value": "red,blue"}]}]})),
        )
        .await
        .unwrap();
        assert_eq!(below, ["heart", "diabetes"]);

        let active = codes(
            vec![code_system()],
            &value_set(json!({"inactive": false, "include": [{"system": CS,
                "filter": [{"property": "concept", "op": "is-a", "value": "heart"}]}]})),
        )
        .await
        .unwrap();
        assert_eq!(active, ["heart", "mi"]);

        let up = codes(
            vec![code_system()],
            &value_set(json!({"include": [{"system": CS,
                "filter": [{"property": "concept", "op": "generalizes", "value": "mi"}]}]})),
        )
        .await
        .unwrap();
        assert_eq!(up, ["heart", "mi"]);
    }

    #[tokio::test]
    async fn enumerated_codes_take_version_and_status_from_the_code_system() {
        let source = Fixtures(vec![code_system()]);
        let expansion = Expander::new(&source)
            .expand(&value_set(json!({"include": [{"system": CS, "concept": [
                {"code": "old-mi"}, {"code": "mi", "display": "Heart attack"}]}]})))
            .await
            .unwrap();
        let rows = value_set_rows(&expansion).unwrap();
        assert_eq!(rows[0]["version"], "3");
        assert_eq!(rows[0]["inactive"], true);
        assert_eq!(rows[1]["display"], "Heart attack");
        assert_eq!(rows[1]["inactive"], Value::Null);
        assert_eq!(
            expansion["parameter"],
            json!([{"name": "version", "valueUri": "http://example.org/cs|3"}])
        );

        // Without the code system the enumeration still determines membership.
        let unknown = codes(
            vec![],
            &value_set(
                json!({"include": [{"system": "http://other", "version": "7",
                "concept": [{"code": "x"}]}]}),
            ),
        )
        .await
        .unwrap();
        assert_eq!(unknown, ["x"]);

        let error = codes(
            vec![code_system()],
            &value_set(json!({"include": [{"system": CS, "concept": [{"code": "nope"}]}]})),
        )
        .await
        .unwrap_err();
        assert!(message(error).contains("not in code system"));
    }

    #[tokio::test]
    async fn imports_intersect_with_the_system_part() {
        let cardiac = json!({"resourceType": "ValueSet", "url": "http://example.org/cardiac",
            "version": "1", "compose": {"include": [{"system": CS,
            "filter": [{"property": "concept", "op": "is-a", "value": "heart"}]}]}});
        let source = Fixtures(vec![code_system(), cardiac]);
        let expansion = Expander::new(&source)
            .expand(&value_set(json!({"include": [
                {"system": CS, "concept": [{"code": "mi"}, {"code": "diabetes"}],
                 "valueSet": ["http://example.org/cardiac"]},
                {"valueSet": ["http://example.org/cardiac|1"]}
            ]})))
            .await
            .unwrap();
        let codes: Vec<&str> = expansion["contains"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, ["mi", "heart", "old-mi"]);
        assert!(expansion["parameter"].as_array().unwrap().contains(
            &json!({"name": "used-valueset", "valueUri": "http://example.org/cardiac|1"})
        ));
    }

    #[tokio::test]
    async fn what_cannot_be_determined_fails() {
        let cases = [
            (
                json!({"include": [{"system": "http://snomed.info/sct",
                    "filter": [{"property": "concept", "op": "is-a", "value": "1"}]}]}),
                "cannot be resolved",
            ),
            (
                json!({"include": [{"valueSet": ["http://example.org/missing"]}]}),
                "cannot be resolved",
            ),
            (
                json!({"include": [{"system": CS,
                    "filter": [{"property": "concept", "op": "is-a-ish", "value": "x"}]}]}),
                "not evaluated",
            ),
            (
                json!({"include": [{"system": CS,
                    "filter": [{"property": "size", "op": "=", "value": "x"}]}]}),
                "does not declare",
            ),
            (
                json!({"include": [{"valueSet": ["http://example.org/vs"]}]}),
                "imports itself",
            ),
            (json!({"include": [{}]}), "neither a system nor a valueSet"),
        ];
        for (compose, expected) in cases {
            let error = codes(vec![code_system()], &value_set(compose.clone()))
                .await
                .unwrap_err();
            let text = message(error);
            assert!(text.contains(expected), "{compose}: {text}");
        }

        let mut fragment = code_system();
        fragment["content"] = json!("fragment");
        let error = codes(
            vec![fragment],
            &value_set(json!({"include": [{"system": CS}]})),
        )
        .await
        .unwrap_err();
        assert!(message(error).contains("content = fragment"));

        let mut paged = value_set(json!({}));
        paged["expansion"] =
            json!({"total": 5, "offset": 0, "contains": [{"system": CS, "code": "mi"}]});
        let error = codes(vec![], &paged).await.unwrap_err();
        assert!(message(error).contains("1 of 5"));
    }
}
