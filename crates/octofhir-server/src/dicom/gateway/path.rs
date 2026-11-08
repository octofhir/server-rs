//! Exact, closed gateway route grammar; a request can never supply a target URL.
//!
//! # Invariants
//! Only canonical UIDs and bounded positive frame lists reach PACS path segments.
use octofhir_api::ApiError;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Studies,
    Series,
    Instances,
    Metadata,
    Object,
    Bulk(String),
}
#[derive(Debug)]
pub(super) struct Target {
    pub study: String,
    pub series: Option<String>,
    pub instance: Option<String>,
    pub suffix: Vec<String>,
    pub kind: Kind,
}
fn invalid() -> ApiError {
    ApiError::bad_request("Unsupported DICOMweb target")
}
impl Target {
    pub fn parse(path: &str, study_query: Option<&str>) -> Result<Self, ApiError> {
        let p: Vec<_> = path.split('/').collect();
        let (study, series, instance, suffix, kind) = match p.as_slice() {
            ["studies"] => (
                study_query.ok_or_else(invalid)?,
                None,
                None,
                vec![],
                Kind::Studies,
            ),
            ["studies", s, "series"] => (*s, None, None, vec![], Kind::Series),
            ["studies", s, "metadata"] => (*s, None, None, vec![], Kind::Metadata),
            ["studies", s, "series", se, "instances"] => {
                (*s, Some(*se), None, vec![], Kind::Instances)
            }
            ["studies", s, "series", se, "metadata"] => {
                (*s, Some(*se), None, vec![], Kind::Metadata)
            }
            ["studies", s, "series", se, "instances", i, tail @ ..] => {
                let kind = match tail {
                    [] => Kind::Object,
                    ["metadata"] => Kind::Metadata,
                    ["pixeldata" | "rendered" | "thumbnail"] => Kind::Object,
                    ["frames", f] | ["frames", f, "rendered" | "thumbnail"] if frames(f) => {
                        Kind::Object
                    }
                    ["bulkdata", token]
                        if !token.is_empty()
                            && token.len() <= 1024
                            && token.bytes().all(|c| c.is_ascii_hexdigit() || c == b'.') =>
                    {
                        Kind::Bulk((*token).into())
                    }
                    _ => return Err(invalid()),
                };
                (
                    *s,
                    Some(*se),
                    Some(*i),
                    tail.iter().map(|v| (*v).into()).collect(),
                    kind,
                )
            }
            _ => return Err(invalid()),
        };
        for uid in [Some(study), series, instance].into_iter().flatten() {
            super::super::validate_uid(uid).map_err(|_| invalid())?;
        }
        Ok(Self {
            study: study.into(),
            series: series.map(Into::into),
            instance: instance.map(Into::into),
            suffix,
            kind,
        })
    }
    pub fn instance_path(&self) -> Vec<&str> {
        vec![
            "studies",
            &self.study,
            "series",
            self.series.as_deref().expect("instance series"),
            "instances",
            self.instance.as_deref().expect("instance UID"),
        ]
    }
}
fn frames(value: &str) -> bool {
    let parts: Vec<_> = value.split(',').collect();
    parts.len() <= 1000
        && parts.iter().all(|v| {
            v.len() <= 10
                && !v.starts_with('0')
                && v.bytes().all(|c| c.is_ascii_digit())
                && v.parse::<u32>().is_ok_and(|n| n > 0)
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_arbitrary_targets_and_frame_traversal() {
        for path in [
            "studies/1.2",
            "studies/1.2/series/1.3",
            "studies/1.2/series/1.3/instances/1.4/frames/0",
            "studies/1.2/series/1.3/instances/1.4/frames/1%2f..",
            "studies/1.2/series/1.3/instances/1.4/bulkdata/https:evil",
        ] {
            assert!(Target::parse(path, None).is_err(), "{path}");
        }
        assert_eq!(
            Target::parse("studies/1.2/series/1.3/instances/1.4/frames/1,2", None)
                .unwrap()
                .kind,
            Kind::Object
        );
    }
}
