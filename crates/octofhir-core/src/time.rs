use crate::error::{CoreError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FhirDateTime(pub OffsetDateTime);

impl FhirDateTime {
    pub fn new(datetime: OffsetDateTime) -> Self {
        Self(datetime)
    }

    pub fn inner(&self) -> &OffsetDateTime {
        &self.0
    }

    pub fn into_inner(self) -> OffsetDateTime {
        self.0
    }

    pub fn timestamp(&self) -> i64 {
        self.0.unix_timestamp()
    }

    pub fn timestamp_nanos(&self) -> i128 {
        self.0.unix_timestamp_nanos()
    }
}

impl fmt::Display for FhirDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted = self
            .0
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| fmt::Error)?;
        write!(f, "{formatted}")
    }
}

impl FromStr for FhirDateTime {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self> {
        let datetime = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                CoreError::invalid_date_time(format!(
                    "Failed to parse FHIR DateTime '{s}': {e}",
                ))
            })?;
        Ok(FhirDateTime(datetime))
    }
}

impl Serialize for FhirDateTime {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let formatted = self
            .0
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&formatted)
    }
}

impl<'de> Deserialize<'de> for FhirDateTime {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FhirDateTime::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub fn now_utc() -> FhirDateTime {
    FhirDateTime(OffsetDateTime::now_utc())
}

pub fn from_unix_timestamp(timestamp: i64) -> Result<FhirDateTime> {
    let datetime = OffsetDateTime::from_unix_timestamp(timestamp)
        .map_err(|e| CoreError::invalid_date_time(format!("Invalid Unix timestamp {timestamp}: {e}")))?;
    Ok(FhirDateTime(datetime))
}

pub fn from_unix_timestamp_nanos(timestamp_nanos: i128) -> Result<FhirDateTime> {
    let datetime = OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos)
        .map_err(|e| CoreError::invalid_date_time(format!("Invalid Unix timestamp nanos {timestamp_nanos}: {e}")))?;
    Ok(FhirDateTime(datetime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn test_fhir_datetime_new() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        assert_eq!(fhir_dt.inner(), &dt);
    }

    #[test]
    fn test_fhir_datetime_into_inner() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        let extracted = fhir_dt.into_inner();
        assert_eq!(extracted, dt);
    }

    #[test]
    fn test_fhir_datetime_display() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        let display = fhir_dt.to_string();
        assert_eq!(display, "2023-05-15T14:30:00Z");
    }

    #[test]
    fn test_fhir_datetime_from_str() {
        let date_str = "2023-05-15T14:30:00Z";
        let fhir_dt = FhirDateTime::from_str(date_str).unwrap();
        let expected = datetime!(2023-05-15 14:30:00 UTC);
        assert_eq!(fhir_dt.0, expected);
    }

    #[test]
    fn test_fhir_datetime_from_str_with_offset() {
        let date_str = "2023-05-15T14:30:00+02:00";
        let fhir_dt = FhirDateTime::from_str(date_str).unwrap();

        let expected_utc = datetime!(2023-05-15 12:30:00 UTC);
        assert_eq!(fhir_dt.0.to_offset(time::UtcOffset::UTC), expected_utc);
    }

    #[test]
    fn test_fhir_datetime_from_str_invalid() {
        assert!(FhirDateTime::from_str("invalid-date").is_err());
        assert!(FhirDateTime::from_str("2023-13-01T00:00:00Z").is_err());
        assert!(FhirDateTime::from_str("2023-01-32T00:00:00Z").is_err());
        assert!(FhirDateTime::from_str("2023-01-01T25:00:00Z").is_err());
        assert!(FhirDateTime::from_str("").is_err());
    }

    #[test]
    fn test_fhir_datetime_serialization() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        let json = serde_json::to_string(&fhir_dt).unwrap();
        assert_eq!(json, "\"2023-05-15T14:30:00Z\"");
    }

    #[test]
    fn test_fhir_datetime_deserialization() {
        let json = "\"2023-05-15T14:30:00Z\"";
        let fhir_dt: FhirDateTime = serde_json::from_str(json).unwrap();
        let expected = datetime!(2023-05-15 14:30:00 UTC);
        assert_eq!(fhir_dt.0, expected);
    }

    #[test]
    fn test_fhir_datetime_deserialization_invalid() {
        let invalid_json = "\"invalid-date\"";
        assert!(serde_json::from_str::<FhirDateTime>(invalid_json).is_err());
    }

    #[test]
    fn test_fhir_datetime_roundtrip() {
        let original = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(original);

        let serialized = serde_json::to_string(&fhir_dt).unwrap();
        let deserialized: FhirDateTime = serde_json::from_str(&serialized).unwrap();

        assert_eq!(fhir_dt, deserialized);
    }

    #[test]
    fn test_now_utc() {
        let now1 = now_utc();
        let now2 = now_utc();

        let diff = now2.0 - now1.0;
        assert!(diff.whole_milliseconds() >= 0);
        assert!(diff.whole_seconds() < 1);
    }

    #[test]
    fn test_fhir_datetime_timestamp() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        let timestamp = fhir_dt.timestamp();
        let expected_timestamp = dt.unix_timestamp();
        assert_eq!(timestamp, expected_timestamp);
    }

    #[test]
    fn test_fhir_datetime_timestamp_nanos() {
        let dt = datetime!(2023-05-15 14:30:00 UTC);
        let fhir_dt = FhirDateTime::new(dt);
        let timestamp_nanos = fhir_dt.timestamp_nanos();
        let expected_timestamp_nanos = dt.unix_timestamp_nanos();
        assert_eq!(timestamp_nanos, expected_timestamp_nanos);
    }

    #[test]
    fn test_from_unix_timestamp() {
        let expected = datetime!(2023-05-15 14:30:00 UTC);
        let timestamp = expected.unix_timestamp();
        let fhir_dt = from_unix_timestamp(timestamp).unwrap();
        assert_eq!(fhir_dt.0, expected);
    }

    #[test]
    fn test_from_unix_timestamp_invalid() {
        let invalid_timestamp = i64::MAX;
        assert!(from_unix_timestamp(invalid_timestamp).is_err());
    }

    #[test]
    fn test_from_unix_timestamp_nanos() {
        let expected = datetime!(2023-05-15 14:30:00 UTC);
        let timestamp_nanos = expected.unix_timestamp_nanos();
        let fhir_dt = from_unix_timestamp_nanos(timestamp_nanos).unwrap();
        assert_eq!(fhir_dt.0, expected);
    }

    #[test]
    fn test_from_unix_timestamp_nanos_invalid() {
        let invalid_timestamp = i128::MAX;
        assert!(from_unix_timestamp_nanos(invalid_timestamp).is_err());
    }

    #[test]
    fn test_fhir_datetime_ordering() {
        let dt1 = FhirDateTime::new(datetime!(2023-05-15 14:30:00 UTC));
        let dt2 = FhirDateTime::new(datetime!(2023-05-15 14:30:01 UTC));
        let dt3 = FhirDateTime::new(datetime!(2023-05-15 14:30:00 UTC));

        assert!(dt1 < dt2);
        assert!(dt2 > dt1);
        assert!(dt1 == dt3);
        assert!(dt1 <= dt3);
        assert!(dt1 >= dt3);
    }

    #[test]
    fn test_fhir_datetime_hash() {
        use std::collections::HashMap;

        let dt1 = FhirDateTime::new(datetime!(2023-05-15 14:30:00 UTC));
        let dt2 = FhirDateTime::new(datetime!(2023-05-15 14:30:00 UTC));
        let dt3 = FhirDateTime::new(datetime!(2023-05-15 14:31:00 UTC));

        let mut map = HashMap::new();
        map.insert(dt1, "first");
        map.insert(dt3, "second");

        assert_eq!(map.get(&dt2), Some(&"first"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_fhir_datetime_debug() {
        let dt = FhirDateTime::new(datetime!(2023-05-15 14:30:00 UTC));
        let debug_str = format!("{dt:?}");
        assert!(debug_str.contains("FhirDateTime"));
    }

    #[test]
    fn test_fhir_datetime_edge_cases() {
        let leap_year_date = "2024-02-29T23:59:59Z";
        let fhir_dt = FhirDateTime::from_str(leap_year_date).unwrap();
        assert_eq!(fhir_dt.to_string(), leap_year_date);

        let microsecond_precision = "2023-05-15T14:30:00.123456Z";
        let fhir_dt = FhirDateTime::from_str(microsecond_precision).unwrap();
        assert!(
            fhir_dt
                .to_string()
                .starts_with("2023-05-15T14:30:00.123456")
        );
    }

    #[test]
    fn test_fhir_datetime_timezone_preservation() {
        let with_tz = "2023-05-15T14:30:00-05:00";
        let fhir_dt = FhirDateTime::from_str(with_tz).unwrap();

        let utc_equivalent = fhir_dt.0.to_offset(time::UtcOffset::UTC);
        let expected_utc = datetime!(2023-05-15 19:30:00 UTC);
        assert_eq!(utc_equivalent, expected_utc);
    }

    #[test]
    fn test_error_message_content() {
        match FhirDateTime::from_str("bad-date") {
            Err(CoreError::InvalidDateTime(msg)) => {
                assert!(msg.contains("bad-date"));
                assert!(msg.contains("Failed to parse FHIR DateTime"));
            }
            _ => panic!("Expected InvalidDateTime error"),
        }
    }
}
