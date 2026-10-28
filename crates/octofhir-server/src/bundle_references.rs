//! Type-aware transaction URI mapping. Canonicals retain their logical identity.

use std::collections::HashMap;
use std::sync::Arc;

use octofhir_api::ApiError;
use octofhir_fhir_model::{ModelProvider, TypeInfo};
use quick_xml::{Reader, Writer, events::Event};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct BundleReferenceRemapper {
    elements: HashMap<String, HashMap<String, Option<Arc<TypeInfo>>>>,
    inherited: HashMap<String, HashMap<String, Arc<TypeInfo>>>,
    types: HashMap<String, Arc<TypeInfo>>,
}

/// One step from a node to its child, kept in an arena so a JSON pointer is
/// only materialised for the few locations that are actually replaced.
enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

impl BundleReferenceRemapper {
    /// Rewrites bundle-local URIs and returns every original Reference.reference
    /// value, so callers need no second walk to find conditional references.
    pub(crate) async fn resolve(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &mut Value,
        aliases: &HashMap<String, String>,
        versions: &HashMap<String, String>,
    ) -> Result<Vec<String>, ApiError> {
        let (replacements, references) = self
            .inspect(provider, resource, aliases, versions, false)
            .await?;
        for (pointer, replacement) in replacements {
            *resource
                .pointer_mut(&pointer)
                .expect("collected resource path") = Value::String(replacement);
        }
        Ok(references)
    }

    pub(crate) async fn references(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &Value,
    ) -> Result<Vec<String>, ApiError> {
        let (_, references) = self
            .inspect(provider, resource, &HashMap::new(), &HashMap::new(), true)
            .await?;
        Ok(references)
    }

    /// Freeze actual Reference.reference locations before a write transaction.
    /// URI-valued identifiers, attachment URLs, narrative and canonical values
    /// are not conditional references, even when their text equals a search URI.
    pub(crate) async fn reference_paths(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &Value,
    ) -> Result<Vec<(String, String)>, ApiError> {
        let (paths, _) = self
            .inspect(provider, resource, &HashMap::new(), &HashMap::new(), true)
            .await?;
        Ok(paths)
    }

    async fn type_of(
        &mut self,
        provider: &dyn ModelProvider,
        name: &str,
    ) -> Result<Option<Arc<TypeInfo>>, ApiError> {
        if let Some(info) = self.types.get(name) {
            return Ok(Some(info.clone()));
        }
        let Some(info) = provider.get_type(name).await.map_err(model_error)? else {
            return Ok(None);
        };
        let info = Arc::new(info);
        self.types.insert(name.to_owned(), info.clone());
        Ok(Some(info))
    }

    async fn element_of(
        &mut self,
        provider: &dyn ModelProvider,
        info: &TypeInfo,
        parent: &str,
        name: &str,
    ) -> Result<Option<Arc<TypeInfo>>, ApiError> {
        if let Some(cached) = self
            .elements
            .get(parent)
            .and_then(|fields| fields.get(name))
        {
            return Ok(cached.clone());
        }
        let mut element = if name.starts_with('_') {
            self.type_of(provider, "Element").await?
        } else {
            provider
                .get_element_type(info, name)
                .await
                .map_err(model_error)?
                .map(Arc::new)
        };
        // Some providers resolve declared fields only. Their element inventory
        // also includes inherited fields (DomainResource.text/extension and
        // Resource.meta).
        if element.is_none() {
            if !self.inherited.contains_key(parent) {
                let fields = provider
                    .get_elements(parent)
                    .await
                    .map_err(model_error)?
                    .into_iter()
                    .filter(|field| {
                        !matches!(field.element_type.as_str(), "Any" | "BackboneElement")
                    })
                    .map(|field| {
                        let info = Arc::new(TypeInfo::new_complex(&field.element_type));
                        (field.name, info)
                    })
                    .collect();
                self.inherited.insert(parent.to_owned(), fields);
            }
            element = self.inherited[parent].get(name).cloned();
        }
        self.elements
            .entry(parent.to_owned())
            .or_default()
            .insert(name.to_owned(), element.clone());
        Ok(element)
    }

    async fn inspect(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &Value,
        aliases: &HashMap<String, String>,
        versions: &HashMap<String, String>,
        collect_only: bool,
    ) -> Result<(Vec<(String, String)>, Vec<String>), ApiError> {
        let resource_type = resource["resourceType"].as_str().unwrap_or_default();
        let Some(root) = self.type_of(provider, resource_type).await? else {
            return Err(ApiError::bad_request("Unavailable Bundle resource model"));
        };
        // arena[i] = (parent index, step from the parent); index 0 is the root.
        let mut arena: Vec<(usize, Step<'_>)> = vec![(0, Step::Index(0))];
        let pointer = |arena: &[(usize, Step<'_>)], mut at: usize| {
            let mut steps = Vec::new();
            while at != 0 {
                steps.push(&arena[at].1);
                at = arena[at].0;
            }
            let mut out = String::new();
            for step in steps.into_iter().rev() {
                out.push('/');
                match step {
                    Step::Key(key) => out.push_str(&key.replace('~', "~0").replace('/', "~1")),
                    Step::Index(index) => out.push_str(&index.to_string()),
                }
            }
            out
        };
        let mut stack: Vec<(&Value, Arc<TypeInfo>, usize)> = vec![(resource, root, 0)];
        let mut replacements = Vec::new();
        let mut references = Vec::new();
        while let Some((node, mut info, at)) = stack.pop() {
            match node {
                Value::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        arena.push((at, Step::Index(index)));
                        stack.push((item, info.clone(), arena.len() - 1));
                    }
                }
                Value::Object(object) => {
                    // Resource-typed fields such as contained and Parameters.resource
                    // carry their own concrete model, rather than Resource's fields.
                    if let Some(resource_type) = object.get("resourceType").and_then(Value::as_str)
                    {
                        info = self
                            .type_of(provider, resource_type)
                            .await?
                            .ok_or_else(|| {
                                ApiError::bad_request("Unavailable embedded resource model")
                            })?;
                    }
                    let parent = info.name.as_deref().unwrap_or(&info.type_name);
                    let is_reference = parent == "Reference";
                    for (name, child) in object {
                        if name == "resourceType" {
                            continue;
                        }
                        let Some(element) = self.element_of(provider, &info, parent, name).await?
                        else {
                            continue;
                        };
                        if let Some(value) = child.as_str() {
                            let reference_field = is_reference && name == "reference";
                            if collect_only {
                                if reference_field {
                                    arena.push((at, Step::Key(name)));
                                    references.push(value.to_owned());
                                    replacements
                                        .push((pointer(&arena, arena.len() - 1), value.to_owned()));
                                }
                                continue;
                            }
                            if reference_field {
                                references.push(value.to_owned());
                            }
                            let kind = element.name.as_deref().unwrap_or(&element.type_name);
                            let mapped = if kind == "xhtml" {
                                remap_narrative(value, aliases, versions)?
                            } else if matches!(kind, "uri" | "url" | "oid" | "uuid")
                                || reference_field
                            {
                                let mapped = remap_uri(value, aliases, versions)?;
                                if mapped.is_none()
                                    && reference_field
                                    && (value.starts_with("urn:uuid:")
                                        || value.starts_with("urn:oid:"))
                                {
                                    return Err(ApiError::bad_request(format!(
                                        "Unresolved reference: {value}"
                                    )));
                                }
                                mapped
                            } else {
                                None
                            };
                            if let Some(mapped) = mapped {
                                arena.push((at, Step::Key(name)));
                                replacements.push((pointer(&arena, arena.len() - 1), mapped));
                            }
                        } else if child.is_object() || child.is_array() {
                            arena.push((at, Step::Key(name)));
                            stack.push((child, element, arena.len() - 1));
                        }
                    }
                }
                // Repeated primitive values use the same element type.
                Value::String(value)
                    if !collect_only
                        && matches!(info.name.as_deref(), Some("uri" | "url" | "oid" | "uuid")) =>
                {
                    if let Some(mapped) = remap_uri(value, aliases, versions)? {
                        replacements.push((pointer(&arena, at), mapped));
                    }
                }
                _ => {}
            }
        }
        Ok((replacements, references))
    }
}

fn model_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("Bundle model lookup failed: {error}"))
}

fn remap_uri(
    value: &str,
    aliases: &HashMap<String, String>,
    versions: &HashMap<String, String>,
) -> Result<Option<String>, ApiError> {
    if let Some(target) = aliases.get(value) {
        return Ok(Some(target.clone()));
    }
    if let Some((source, fragment)) = value.split_once('#')
        && let Some(mapped) = remap_uri(source, aliases, versions)?
    {
        return Ok(Some(format!("{mapped}#{fragment}")));
    }

    if let Some((source, old_version)) = value.rsplit_once("/_history/")
        && let Some(target) = aliases.get(source)
    {
        octofhir_core::validate_id(old_version)
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        let version = versions
            .get(source)
            .ok_or_else(|| ApiError::internal("Missing planned reference version"))?;
        return Ok(Some(format!("{target}/_history/{version}")));
    }
    Ok(None)
}

fn remap_narrative(
    narrative: &str,
    aliases: &HashMap<String, String>,
    versions: &HashMap<String, String>,
) -> Result<Option<String>, ApiError> {
    let mut reader = Reader::from_str(narrative);
    let mut writer = Writer::new(Vec::new());
    let mut changed = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| ApiError::bad_request(format!("Invalid narrative XML: {e}")))?;
        let event = match event {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let empty = matches!(&event, Event::Empty(_));
                let tag = match &event {
                    Event::Start(tag) | Event::Empty(tag) => tag,
                    _ => unreachable!(),
                };
                let mut output = tag.to_owned();
                let tag_name = tag.local_name();
                let attribute_name = match tag_name.as_ref() {
                    b"a" => Some(b"href".as_slice()),
                    b"img" => Some(b"src".as_slice()),
                    _ => None,
                };
                if let Some(attribute_name) = attribute_name {
                    output.clear_attributes();
                    for attribute in tag.attributes() {
                        let attribute = attribute.map_err(|e| {
                            ApiError::bad_request(format!("Invalid narrative attribute: {e}"))
                        })?;
                        if attribute.key.as_ref() == attribute_name {
                            let value = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(|e| {
                                    ApiError::bad_request(format!("Invalid narrative URI: {e}"))
                                })?;
                            if let Some(target) = remap_uri(&value, aliases, versions)? {
                                let key = std::str::from_utf8(attribute.key.as_ref())
                                    .expect("ASCII narrative attribute");
                                output.push_attribute((key, target.as_str()));
                                changed = true;
                                continue;
                            }
                        }
                        output.push_attribute(attribute);
                    }
                }
                if empty {
                    Event::Empty(output)
                } else {
                    Event::Start(output)
                }
            }
            Event::Eof => break,
            other => other.into_owned(),
        };
        writer
            .write_event(event)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    if changed {
        String::from_utf8(writer.into_inner())
            .map(Some)
            .map_err(|e| ApiError::internal(e.to_string()))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn typed_uri_version_and_narrative_mapping_preserves_canonicals_and_text() {
        let provider = octofhir_fhirschema::FhirSchemaModelProvider::new(
            octofhir_fhirschema::embedded::get_schemas(
                octofhir_fhirschema::embedded::FhirVersion::R4,
            )
            .clone(),
            octofhir_fhir_model::FhirVersion::R4,
        );
        let source = "urn:uuid:550e8400-e29b-41d4-a716-446655440000";
        let aliases = HashMap::from([(source.to_owned(), "Patient/new-id".to_owned())]);
        let versions = HashMap::from([(source.to_owned(), "99".to_owned())]);
        let mut body = json!({"resourceType":"Patient","id":"new-id",
            "meta":{"profile":[source]},
            "identifier":[{"system":source,"value":source}],
            "photo":[{"url":source,"title":source}],
            "generalPractitioner":[{"reference":format!("{source}/_history/arbitrary-old-version")}],
            "extension":[{"url":"urn:test:uri","valueUri":source},{"url":"urn:test:canonical","valueCanonical":source}],
            "_birthDate":{"extension":[{"url":"urn:test:primitive","valueUrl":source}]},
            "text":{"status":"generated","div":format!("<div xmlns='http://www.w3.org/1999/xhtml'><a title='{source}' href='{source}/_history/old#part'>link</a><img src='{source}' /></div>")}
        });
        let mut mapper = BundleReferenceRemapper::default();
        mapper
            .resolve(&provider, &mut body, &aliases, &versions)
            .await
            .unwrap();
        assert_eq!(
            mapper.references(&provider, &body).await.unwrap(),
            vec!["Patient/new-id/_history/99"]
        );
        assert_eq!(body["meta"]["profile"][0], source);
        assert_eq!(body["identifier"][0]["system"], "Patient/new-id");
        assert_eq!(body["identifier"][0]["value"], source);
        assert_eq!(body["photo"][0]["url"], "Patient/new-id");
        assert_eq!(body["photo"][0]["title"], source);
        assert_eq!(
            body["generalPractitioner"][0]["reference"],
            "Patient/new-id/_history/99"
        );
        assert_eq!(body["extension"][0]["valueUri"], "Patient/new-id");
        assert_eq!(body["extension"][1]["valueCanonical"], source);
        assert_eq!(
            body["_birthDate"]["extension"][0]["valueUrl"],
            "Patient/new-id"
        );
        let narrative = body["text"]["div"].as_str().unwrap();
        assert!(
            narrative.contains("href=\"Patient/new-id/_history/99#part\""),
            "{narrative}"
        );
        assert!(narrative.contains("src=\"Patient/new-id\""), "{narrative}");
        assert!(
            narrative.contains(&format!("title=\"{source}\"")),
            "{narrative}"
        );
        let unchanged = body.clone();
        mapper
            .resolve(&provider, &mut body, &aliases, &versions)
            .await
            .unwrap();
        assert_eq!(body, unchanged);
    }

    #[test]
    fn narrative_attributes_decode_entities_and_escape_new_targets() {
        let aliases = HashMap::from([(
            "https://source.test/Patient/old?a=1&b=2".into(),
            "https://target.test/Patient/new?a=3&b=4".into(),
        )]);
        let source = "<div xmlns=\"http://www.w3.org/1999/xhtml\"><a href='https://source.test/Patient/old?a=1&amp;b=2'>x</a></div>";
        let mapped = remap_narrative(source, &aliases, &HashMap::new())
            .unwrap()
            .unwrap();
        assert!(
            mapped.contains("href=\"https://target.test/Patient/new?a=3&amp;b=4\""),
            "{mapped}"
        );
        assert_eq!(
            remap_narrative(source, &HashMap::new(), &HashMap::new()).unwrap(),
            None
        );
    }
}
