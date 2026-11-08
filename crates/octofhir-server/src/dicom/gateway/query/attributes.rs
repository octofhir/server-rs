//! Supported QIDO information-entity attributes from PS3.18 10.6.
//!
//! # Contents
//! Required/conditional projections, supported optional fields and keyword paths.
//!
//! # Invariants
//! Patient attributes belong to the Study level. Instance attributes never appear
//! in Study/Series projections. Required missing values retain their DICOM VR.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Level {
    Study,
    Series,
    Instance,
}
pub(super) struct Attribute {
    pub name: &'static str,
    pub tag: &'static str,
    pub vr: &'static str,
    pub level: Level,
    pub default: u8,
}
use Level::*;
pub(super) const ATTRIBUTES: &[Attribute] = &[
    Attribute {
        name: "SpecificCharacterSet",
        tag: "00080005",
        vr: "CS",
        level: Study,
        default: 2,
    },
    Attribute {
        name: "StudyDate",
        tag: "00080020",
        vr: "DA",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "StudyTime",
        tag: "00080030",
        vr: "TM",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "AccessionNumber",
        tag: "00080050",
        vr: "SH",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "ModalitiesInStudy",
        tag: "00080061",
        vr: "CS",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "ReferringPhysicianName",
        tag: "00080090",
        vr: "PN",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "StudyDescription",
        tag: "00081030",
        vr: "LO",
        level: Study,
        default: 0,
    },
    Attribute {
        name: "PatientName",
        tag: "00100010",
        vr: "PN",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "PatientID",
        tag: "00100020",
        vr: "LO",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "IssuerOfPatientID",
        tag: "00100021",
        vr: "LO",
        level: Study,
        default: 0,
    },
    Attribute {
        name: "PatientBirthDate",
        tag: "00100030",
        vr: "DA",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "PatientSex",
        tag: "00100040",
        vr: "CS",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "StudyInstanceUID",
        tag: "0020000D",
        vr: "UI",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "StudyID",
        tag: "00200010",
        vr: "SH",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "NumberOfStudyRelatedSeries",
        tag: "00201206",
        vr: "IS",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "NumberOfStudyRelatedInstances",
        tag: "00201208",
        vr: "IS",
        level: Study,
        default: 1,
    },
    Attribute {
        name: "Modality",
        tag: "00080060",
        vr: "CS",
        level: Series,
        default: 1,
    },
    Attribute {
        name: "SeriesDescription",
        tag: "0008103E",
        vr: "LO",
        level: Series,
        default: 2,
    },
    Attribute {
        name: "SeriesInstanceUID",
        tag: "0020000E",
        vr: "UI",
        level: Series,
        default: 1,
    },
    Attribute {
        name: "SeriesNumber",
        tag: "00200011",
        vr: "IS",
        level: Series,
        default: 1,
    },
    Attribute {
        name: "NumberOfSeriesRelatedInstances",
        tag: "00201209",
        vr: "IS",
        level: Series,
        default: 1,
    },
    Attribute {
        name: "PerformedProcedureStepStartDate",
        tag: "00400244",
        vr: "DA",
        level: Series,
        default: 2,
    },
    Attribute {
        name: "PerformedProcedureStepStartTime",
        tag: "00400245",
        vr: "TM",
        level: Series,
        default: 2,
    },
    Attribute {
        name: "RequestAttributesSequence",
        tag: "00400275",
        vr: "SQ",
        level: Series,
        default: 2,
    },
    Attribute {
        name: "SOPClassUID",
        tag: "00080016",
        vr: "UI",
        level: Instance,
        default: 1,
    },
    Attribute {
        name: "SOPInstanceUID",
        tag: "00080018",
        vr: "UI",
        level: Instance,
        default: 1,
    },
    Attribute {
        name: "InstanceNumber",
        tag: "00200013",
        vr: "IS",
        level: Instance,
        default: 1,
    },
    Attribute {
        name: "Rows",
        tag: "00280010",
        vr: "US",
        level: Instance,
        default: 2,
    },
    Attribute {
        name: "Columns",
        tag: "00280011",
        vr: "US",
        level: Instance,
        default: 2,
    },
    Attribute {
        name: "BitsAllocated",
        tag: "00280100",
        vr: "US",
        level: Instance,
        default: 2,
    },
    Attribute {
        name: "NumberOfFrames",
        tag: "00280008",
        vr: "IS",
        level: Instance,
        default: 2,
    },
    Attribute {
        name: "InstanceAvailability",
        tag: "00080056",
        vr: "CS",
        level: Study,
        default: 2,
    },
    Attribute {
        name: "TimezoneOffsetFromUTC",
        tag: "00080201",
        vr: "SH",
        level: Study,
        default: 2,
    },
    Attribute {
        name: "RetrieveURL",
        tag: "00081190",
        vr: "UR",
        level: Study,
        default: 2,
    },
];
pub(super) fn attribute(name: &str) -> Option<&'static Attribute> {
    ATTRIBUTES
        .iter()
        .find(|a| a.name == name || a.tag.eq_ignore_ascii_case(name))
}
pub(super) fn path(name: &str) -> Option<Vec<String>> {
    let parts = name.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        [single] => Some(vec![attribute(single)?.tag.into()]),
        [sequence, child] if attribute(sequence)?.tag == "00400275" => Some(vec![
            "00400275".into(),
            match *child {
                "ScheduledProcedureStepID" | "00400009" => "00400009",
                "RequestedProcedureID" | "00401001" => "00401001",
                _ => return None,
            }
            .into(),
        ]),
        _ => None,
    }
}
