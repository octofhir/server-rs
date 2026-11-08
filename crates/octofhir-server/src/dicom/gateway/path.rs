//! Exact, closed gateway route grammar; a request can never supply a target URL.
//!
//! # Invariants
//! Only canonical UIDs and strictly ascending bounded frame lists reach PACS.
use octofhir_api::ApiError;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Studies,
    Series,
    Instances,
    Metadata,
    Object,
    Collection,
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
        if path.len() > 16384 {
            return Err(invalid());
        }
        let p: Vec<_> = path.split('/').collect();
        let (study, series, instance, suffix, kind) = match p.as_slice() {
            ["studies"] => (study_query.unwrap_or(""), None, None, vec![], Kind::Studies),
            ["series"] => (study_query.unwrap_or(""), None, None, vec![], Kind::Series),
            ["instances"] => (
                study_query.unwrap_or(""),
                None,
                None,
                vec![],
                Kind::Instances,
            ),
            ["studies", s, "instances"] => (*s, None, None, vec![], Kind::Instances),
            ["studies", s] => (*s, None, None, vec![], Kind::Collection),
            [
                "studies",
                s,
                suffix @ ("rendered" | "thumbnail" | "pixeldata"),
            ] => (*s, None, None, vec![(*suffix).into()], Kind::Collection),
            ["studies", s, "series"] => (*s, None, None, vec![], Kind::Series),
            ["studies", s, "metadata"] => (*s, None, None, vec![], Kind::Metadata),
            ["studies", s, "series", se, "instances"] => {
                (*s, Some(*se), None, vec![], Kind::Instances)
            }
            ["studies", s, "series", se, "metadata"] => {
                (*s, Some(*se), None, vec![], Kind::Metadata)
            }
            ["studies", s, "series", se] => (*s, Some(*se), None, vec![], Kind::Collection),
            [
                "studies",
                s,
                "series",
                se,
                suffix @ ("rendered" | "thumbnail" | "pixeldata"),
            ] => (
                *s,
                Some(*se),
                None,
                vec![(*suffix).into()],
                Kind::Collection,
            ),
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
            if uid.is_empty() && matches!(kind, Kind::Studies | Kind::Series | Kind::Instances) {
                continue;
            }
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
}
fn frames(value: &str) -> bool {
    if value.len() > 10999 {
        return false;
    }
    let parts: Vec<_> = value.split(',').collect();
    let mut previous = 0;
    parts.len() <= 1000
        && parts.iter().all(|v| {
            v.len() <= 10
                && !v.starts_with('0')
                && v.bytes().all(|c| c.is_ascii_digit())
                && v.parse::<u32>().is_ok_and(|n| {
                    let valid = n > previous;
                    previous = n;
                    valid
                })
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_arbitrary_targets_and_frame_traversal() {
        for path in [
            "studies/1.2/series/1.3/instances/1.4/frames/2,1",
            "studies/1.2/series/1.3/instances/1.4/frames/1,1",
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
