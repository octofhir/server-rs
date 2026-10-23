//! Type-aware transaction URI mapping. Canonicals retain their logical identity.

use std::collections::HashMap;

use octofhir_api::ApiError;
use octofhir_fhir_model::{ModelProvider, TypeInfo};
use quick_xml::{Reader, Writer, events::Event};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct BundleReferenceRemapper {
    elements: HashMap<(String, String), Option<TypeInfo>>,
    inherited: HashMap<String, HashMap<String, TypeInfo>>,
}

impl BundleReferenceRemapper {
    pub(crate) async fn resolve(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &mut Value,
        aliases: &HashMap<String, String>,
        versions: &HashMap<String, String>,
    ) -> Result<(), ApiError> {
        let (replacements, _) = self
            .inspect(provider, resource, aliases, versions, false)
            .await?;
        for (pointer, replacement) in replacements {
            *resource
                .pointer_mut(&pointer)
                .expect("collected resource path") = Value::String(replacement);
        }
        Ok(())
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

    async fn inspect(
        &mut self,
        provider: &dyn ModelProvider,
        resource: &Value,
        aliases: &HashMap<String, String>,
        versions: &HashMap<String, String>,
        collect_only: bool,
    ) -> Result<(Vec<(String, String)>, Vec<String>), ApiError> {
        let resource_type = resource["resourceType"].as_str().unwrap_or_default();
        let Some(root) = provider
            .get_type(resource_type)
            .await
            .map_err(model_error)?
        else {
            return Err(ApiError::bad_request("Unavailable Bundle resource model"));
        };
        let mut stack = vec![(String::new(), root)];
        let mut replacements = Vec::new();
        let mut references = Vec::new();
        while let Some((pointer, mut info)) = stack.pop() {
            let node = resource.pointer(&pointer).expect("collected resource path");
            match node {
                Value::Array(items) => {
                    for index in 0..items.len() {
                        stack.push((format!("{pointer}/{index}"), info.clone()));
                    }
                }
                Value::Object(object) => {
                    // Resource-typed fields such as contained and Parameters.resource
                    // carry their own concrete model, rather than Resource's fields.
                    if let Some(resource_type) = object.get("resourceType").and_then(Value::as_str)
                    {
                        info = provider
                            .get_type(resource_type)
                            .await
                            .map_err(model_error)?
                            .ok_or_else(|| {
                                ApiError::bad_request("Unavailable embedded resource model")
                            })?;
                    }
                    let parent = info.name.as_deref().unwrap_or(&info.type_name);
                    for (name, child) in object {
                        if name == "resourceType" {
                            continue;
                        }
                        let key = (parent.to_owned(), name.clone());
                        if !self.elements.contains_key(&key) {
                            let mut element = if name.starts_with('_') {
                                provider.get_type("Element").await.map_err(model_error)?
                            } else {
                                provider
                                    .get_element_type(&info, name)
                                    .await
                                    .map_err(model_error)?
                            };
                            // Some providers resolve declared fields only. Their
                            // element inventory also includes inherited fields
                            // (DomainResource.text/extension and Resource.meta).
                            if element.is_none() {
                                if !self.inherited.contains_key(parent) {
                                    let fields = provider
                                        .get_elements(parent)
                                        .await
                                        .map_err(model_error)?
                                        .into_iter()
                                        .filter(|field| {
                                            !matches!(
                                                field.element_type.as_str(),
                                                "Any" | "BackboneElement"
                                            )
                                        })
                                        .map(|field| {
                                            (field.name, TypeInfo::new_complex(&field.element_type))
                                        })
                                        .collect();
                                    self.inherited.insert(parent.to_owned(), fields);
                                }
                                element = self.inherited[parent].get(name).cloned();
                            }
                            self.elements.insert(key.clone(), element);
                        }
                        let Some(element) = self.elements[&key].as_ref() else {
                            continue;
                        };
                        let path =
                            format!("{pointer}/{}", name.replace('~', "~0").replace('/', "~1"));
                        let kind = element.name.as_deref().unwrap_or(&element.type_name);
                        if let Some(value) = child.as_str() {
                            if collect_only {
                                if parent == "Reference" && name == "reference" {
                                    references.push(value.to_owned());
                                    replacements.push((path, value.to_owned()));
                                }
                                continue;
                            }
                            if kind == "xhtml" {
                                if let Some(mapped) = remap_narrative(value, aliases, versions)? {
                                    replacements.push((path, mapped));
                                }
                            } else if matches!(kind, "uri" | "url" | "oid" | "uuid")
                                || (parent == "Reference" && name == "reference")
                            {
                                if let Some(mapped) = remap_uri(value, aliases, versions)? {
                                    replacements.push((path, mapped));
                                } else if parent == "Reference"
                                    && name == "reference"
                                    && (value.starts_with("urn:uuid:")
                                        || value.starts_with("urn:oid:"))
                                {
                                    return Err(ApiError::bad_request(format!(
                                        "Unresolved reference: {value}"
                                    )));
                                }
                            }
                        } else if child.is_object() || child.is_array() {
                            stack.push((path, element.clone()));
                        }
                    }
                }
                // Repeated primitive values use the same element type.
                Value::String(value)
                    if !collect_only
                        && matches!(info.name.as_deref(), Some("uri" | "url" | "oid" | "uuid")) =>
                {
                    if let Some(mapped) = remap_uri(value, aliases, versions)? {
                        replacements.push((pointer, mapped));
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
