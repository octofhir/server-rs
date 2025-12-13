//! FHIR Compartment-based access control.
//!
//! This module provides compartment-based access control that restricts access to
//! resources based on compartment membership (e.g., Patient compartment).
//!
//! # Overview
//!
//! FHIR compartments define logical groupings of resources related to a specific
//! context. The most common use case is the Patient compartment, which groups all
//! resources related to a specific patient.
//!
//! # Standard Compartments
//!
//! - **Patient**: All resources related to a patient
//! - **Practitioner**: All resources related to a practitioner
//! - **Encounter**: All resources related to an encounter
//! - **RelatedPerson**: All resources related to a related person
//! - **Device**: All resources related to a device
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_auth::policy::compartment::CompartmentChecker;
//!
//! let checker = CompartmentChecker::new();
//!
//! let observation = serde_json::json!({
//!     "resourceType": "Observation",
//!     "subject": {"reference": "Patient/123"}
//! });
//!
//! assert!(checker.is_in_compartment("Patient", "123", &observation));
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::context::PolicyContext;
use super::engine::{AccessDecision, DenyReason};
use crate::smart::scopes::FhirOperation;

// =============================================================================
// Compartment Definition
// =============================================================================

/// How a resource is included in a compartment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentInclusion {
    /// Search parameter that links to compartment.
    pub param: String,

    /// Optional FHIRPath expression for complex cases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhirpath: Option<String>,
}

/// FHIR compartment definition.
#[derive(Debug, Clone)]
pub struct CompartmentDefinition {
    /// Compartment type (e.g., "Patient", "Practitioner").
    pub code: String,

    /// Resources included in this compartment and their inclusion criteria.
    pub resources: HashMap<String, Vec<CompartmentInclusion>>,
}

impl CompartmentDefinition {
    /// Create the standard Patient compartment definition.
    ///
    /// Based on: <https://build.fhir.org/compartmentdefinition-patient.html>
    #[must_use]
    pub fn patient() -> Self {
        let mut resources = HashMap::new();

        // Patient - self-reference
        resources.insert(
            "Patient".to_string(),
            vec![CompartmentInclusion {
                param: "{def}".to_string(),
                fhirpath: None,
            }],
        );

        // Account
        resources.insert(
            "Account".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // AllergyIntolerance
        resources.insert(
            "AllergyIntolerance".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recorder".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "asserter".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Appointment
        resources.insert(
            "Appointment".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // AppointmentResponse
        resources.insert(
            "AppointmentResponse".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // AuditEvent
        resources.insert(
            "AuditEvent".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Basic
        resources.insert(
            "Basic".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // BodyStructure
        resources.insert(
            "BodyStructure".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // CarePlan
        resources.insert(
            "CarePlan".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // CareTeam
        resources.insert(
            "CareTeam".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "participant".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // ChargeItem
        resources.insert(
            "ChargeItem".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "enterer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer-actor".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Claim
        resources.insert(
            "Claim".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "payee".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // ClaimResponse
        resources.insert(
            "ClaimResponse".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // ClinicalImpression
        resources.insert(
            "ClinicalImpression".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // Communication
        resources.insert(
            "Communication".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "sender".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // CommunicationRequest
        resources.insert(
            "CommunicationRequest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "sender".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "requester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Composition
        resources.insert(
            "Composition".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "attester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Condition - uses "subject" field but "patient" search param
        resources.insert(
            "Condition".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: Some("subject".to_string()),
                },
                CompartmentInclusion {
                    param: "asserter".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Consent
        resources.insert(
            "Consent".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Coverage
        resources.insert(
            "Coverage".to_string(),
            vec![
                CompartmentInclusion {
                    param: "policy-holder".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "subscriber".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "beneficiary".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "payor".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // DetectedIssue
        resources.insert(
            "DetectedIssue".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // DeviceRequest
        resources.insert(
            "DeviceRequest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // DeviceUseStatement
        resources.insert(
            "DeviceUseStatement".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // DiagnosticReport
        resources.insert(
            "DiagnosticReport".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // DocumentManifest
        resources.insert(
            "DocumentManifest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // DocumentReference
        resources.insert(
            "DocumentReference".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Encounter
        resources.insert(
            "Encounter".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // EnrollmentRequest
        resources.insert(
            "EnrollmentRequest".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // EpisodeOfCare
        resources.insert(
            "EpisodeOfCare".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // ExplanationOfBenefit
        resources.insert(
            "ExplanationOfBenefit".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "payee".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // FamilyMemberHistory
        resources.insert(
            "FamilyMemberHistory".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Flag
        resources.insert(
            "Flag".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Goal
        resources.insert(
            "Goal".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Group
        resources.insert(
            "Group".to_string(),
            vec![CompartmentInclusion {
                param: "member".to_string(),
                fhirpath: None,
            }],
        );

        // ImagingStudy
        resources.insert(
            "ImagingStudy".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Immunization
        resources.insert(
            "Immunization".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // ImmunizationEvaluation
        resources.insert(
            "ImmunizationEvaluation".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // ImmunizationRecommendation
        resources.insert(
            "ImmunizationRecommendation".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Invoice
        resources.insert(
            "Invoice".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // List
        resources.insert(
            "List".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "source".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MeasureReport
        resources.insert(
            "MeasureReport".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Media
        resources.insert(
            "Media".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // MedicationAdministration
        resources.insert(
            "MedicationAdministration".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MedicationDispense
        resources.insert(
            "MedicationDispense".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "receiver".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MedicationRequest
        resources.insert(
            "MedicationRequest".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // MedicationStatement
        resources.insert(
            "MedicationStatement".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "source".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MolecularSequence
        resources.insert(
            "MolecularSequence".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // NutritionOrder
        resources.insert(
            "NutritionOrder".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // Observation
        resources.insert(
            "Observation".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Procedure
        resources.insert(
            "Procedure".to_string(),
            vec![
                CompartmentInclusion {
                    param: "patient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Provenance
        resources.insert(
            "Provenance".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // QuestionnaireResponse
        resources.insert(
            "QuestionnaireResponse".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "source".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // RelatedPerson
        resources.insert(
            "RelatedPerson".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // RequestGroup
        resources.insert(
            "RequestGroup".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "participant".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // ResearchSubject
        resources.insert(
            "ResearchSubject".to_string(),
            vec![CompartmentInclusion {
                param: "individual".to_string(),
                fhirpath: None,
            }],
        );

        // RiskAssessment
        resources.insert(
            "RiskAssessment".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // Schedule
        resources.insert(
            "Schedule".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // ServiceRequest
        resources.insert(
            "ServiceRequest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Specimen
        resources.insert(
            "Specimen".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // SupplyDelivery
        resources.insert(
            "SupplyDelivery".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        // SupplyRequest
        resources.insert(
            "SupplyRequest".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // VisionPrescription
        resources.insert(
            "VisionPrescription".to_string(),
            vec![CompartmentInclusion {
                param: "patient".to_string(),
                fhirpath: None,
            }],
        );

        Self {
            code: "Patient".to_string(),
            resources,
        }
    }

    /// Create the standard Practitioner compartment definition.
    ///
    /// Based on: <https://build.fhir.org/compartmentdefinition-practitioner.html>
    #[must_use]
    pub fn practitioner() -> Self {
        let mut resources = HashMap::new();

        // Practitioner - self-reference
        resources.insert(
            "Practitioner".to_string(),
            vec![CompartmentInclusion {
                param: "{def}".to_string(),
                fhirpath: None,
            }],
        );

        // Account
        resources.insert(
            "Account".to_string(),
            vec![CompartmentInclusion {
                param: "subject".to_string(),
                fhirpath: None,
            }],
        );

        // Appointment
        resources.insert(
            "Appointment".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // AppointmentResponse
        resources.insert(
            "AppointmentResponse".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // AuditEvent
        resources.insert(
            "AuditEvent".to_string(),
            vec![CompartmentInclusion {
                param: "agent".to_string(),
                fhirpath: None,
            }],
        );

        // CarePlan
        resources.insert(
            "CarePlan".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // CareTeam
        resources.insert(
            "CareTeam".to_string(),
            vec![CompartmentInclusion {
                param: "participant".to_string(),
                fhirpath: None,
            }],
        );

        // Claim
        resources.insert(
            "Claim".to_string(),
            vec![
                CompartmentInclusion {
                    param: "enterer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "provider".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "payee".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "care-team".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Communication
        resources.insert(
            "Communication".to_string(),
            vec![
                CompartmentInclusion {
                    param: "sender".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // CommunicationRequest
        resources.insert(
            "CommunicationRequest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "sender".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "requester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Composition
        resources.insert(
            "Composition".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "attester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Condition
        resources.insert(
            "Condition".to_string(),
            vec![CompartmentInclusion {
                param: "asserter".to_string(),
                fhirpath: None,
            }],
        );

        // DiagnosticReport
        resources.insert(
            "DiagnosticReport".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // DocumentManifest
        resources.insert(
            "DocumentManifest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "recipient".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // DocumentReference
        resources.insert(
            "DocumentReference".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "authenticator".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Encounter
        resources.insert(
            "Encounter".to_string(),
            vec![
                CompartmentInclusion {
                    param: "practitioner".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "participant".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // EpisodeOfCare
        resources.insert(
            "EpisodeOfCare".to_string(),
            vec![CompartmentInclusion {
                param: "care-manager".to_string(),
                fhirpath: None,
            }],
        );

        // ExplanationOfBenefit
        resources.insert(
            "ExplanationOfBenefit".to_string(),
            vec![
                CompartmentInclusion {
                    param: "enterer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "provider".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "payee".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "care-team".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Flag
        resources.insert(
            "Flag".to_string(),
            vec![CompartmentInclusion {
                param: "author".to_string(),
                fhirpath: None,
            }],
        );

        // Group
        resources.insert(
            "Group".to_string(),
            vec![
                CompartmentInclusion {
                    param: "member".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "managing-entity".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // ImagingStudy
        resources.insert(
            "ImagingStudy".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // Immunization
        resources.insert(
            "Immunization".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // List
        resources.insert(
            "List".to_string(),
            vec![CompartmentInclusion {
                param: "source".to_string(),
                fhirpath: None,
            }],
        );

        // MeasureReport
        resources.insert(
            "MeasureReport".to_string(),
            vec![CompartmentInclusion {
                param: "reporter".to_string(),
                fhirpath: None,
            }],
        );

        // Media
        resources.insert(
            "Media".to_string(),
            vec![
                CompartmentInclusion {
                    param: "subject".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "operator".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MedicationAdministration
        resources.insert(
            "MedicationAdministration".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // MedicationDispense
        resources.insert(
            "MedicationDispense".to_string(),
            vec![
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "receiver".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // MedicationRequest
        resources.insert(
            "MedicationRequest".to_string(),
            vec![CompartmentInclusion {
                param: "requester".to_string(),
                fhirpath: None,
            }],
        );

        // MedicationStatement
        resources.insert(
            "MedicationStatement".to_string(),
            vec![CompartmentInclusion {
                param: "source".to_string(),
                fhirpath: None,
            }],
        );

        // Observation
        resources.insert(
            "Observation".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // Procedure
        resources.insert(
            "Procedure".to_string(),
            vec![CompartmentInclusion {
                param: "performer".to_string(),
                fhirpath: None,
            }],
        );

        // Provenance
        resources.insert(
            "Provenance".to_string(),
            vec![CompartmentInclusion {
                param: "agent".to_string(),
                fhirpath: None,
            }],
        );

        // QuestionnaireResponse
        resources.insert(
            "QuestionnaireResponse".to_string(),
            vec![
                CompartmentInclusion {
                    param: "author".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "source".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Schedule
        resources.insert(
            "Schedule".to_string(),
            vec![CompartmentInclusion {
                param: "actor".to_string(),
                fhirpath: None,
            }],
        );

        // ServiceRequest
        resources.insert(
            "ServiceRequest".to_string(),
            vec![
                CompartmentInclusion {
                    param: "performer".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "requester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        // Slot
        resources.insert(
            "Slot".to_string(),
            vec![CompartmentInclusion {
                param: "schedule".to_string(),
                fhirpath: None,
            }],
        );

        // SupplyRequest
        resources.insert(
            "SupplyRequest".to_string(),
            vec![CompartmentInclusion {
                param: "requester".to_string(),
                fhirpath: None,
            }],
        );

        // Task
        resources.insert(
            "Task".to_string(),
            vec![
                CompartmentInclusion {
                    param: "owner".to_string(),
                    fhirpath: None,
                },
                CompartmentInclusion {
                    param: "requester".to_string(),
                    fhirpath: None,
                },
            ],
        );

        Self {
            code: "Practitioner".to_string(),
            resources,
        }
    }

    /// Check if a resource type is included in this compartment.
    #[must_use]
    pub fn includes_resource_type(&self, resource_type: &str) -> bool {
        self.resources.contains_key(resource_type)
    }

    /// Get inclusion criteria for a resource type.
    #[must_use]
    pub fn get_inclusions(&self, resource_type: &str) -> Option<&Vec<CompartmentInclusion>> {
        self.resources.get(resource_type)
    }
}

// =============================================================================
// Compartment Checker
// =============================================================================

/// Service for checking compartment membership.
pub struct CompartmentChecker {
    patient_compartment: CompartmentDefinition,
    practitioner_compartment: CompartmentDefinition,
}

impl Default for CompartmentChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl CompartmentChecker {
    /// Create a new compartment checker with standard definitions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            patient_compartment: CompartmentDefinition::patient(),
            practitioner_compartment: CompartmentDefinition::practitioner(),
        }
    }

    /// Check if a resource is in the specified compartment.
    #[must_use]
    pub fn is_in_compartment(
        &self,
        compartment_type: &str,
        compartment_id: &str,
        resource: &serde_json::Value,
    ) -> bool {
        let definition = match compartment_type {
            "Patient" => &self.patient_compartment,
            "Practitioner" => &self.practitioner_compartment,
            _ => return false,
        };

        let resource_type = resource["resourceType"].as_str().unwrap_or("");

        // Check if resource type is in compartment
        let Some(inclusions) = definition.get_inclusions(resource_type) else {
            return false;
        };

        // Special case: the compartment resource itself
        if resource_type == compartment_type {
            let resource_id = resource["id"].as_str().unwrap_or("");
            return resource_id == compartment_id;
        }

        // Check each inclusion criterion
        for inclusion in inclusions {
            if inclusion.param == "{def}" {
                continue; // Self-reference, handled above
            }

            if self.check_inclusion(compartment_type, compartment_id, resource, inclusion) {
                return true;
            }
        }

        false
    }

    /// Check a single inclusion criterion.
    fn check_inclusion(
        &self,
        compartment_type: &str,
        compartment_id: &str,
        resource: &serde_json::Value,
        inclusion: &CompartmentInclusion,
    ) -> bool {
        let expected_ref = format!("{}/{}", compartment_type, compartment_id);

        // Use fhirpath if provided, otherwise use param name
        // This handles cases like Condition.subject where search param is "patient"
        let field_path = inclusion.fhirpath.as_deref().unwrap_or(&inclusion.param);

        // Get the reference value(s) from the resource
        let references = self.extract_references(resource, field_path);

        references.iter().any(|r| {
            r == &expected_ref
                || r == compartment_id
                || r.ends_with(&format!("/{}", compartment_id))
        })
    }

    /// Extract all reference values for a given parameter.
    fn extract_references(&self, resource: &serde_json::Value, param: &str) -> Vec<String> {
        let mut refs = Vec::new();

        // Handle direct field access
        if let Some(value) = resource.get(param) {
            Self::collect_references(value, &mut refs);
        }

        refs
    }

    /// Collect reference strings from a value.
    fn collect_references(value: &serde_json::Value, refs: &mut Vec<String>) {
        // Direct string reference
        if let Some(s) = value.as_str() {
            refs.push(s.to_string());
            return;
        }

        // Reference object with .reference field
        if let Some(ref_value) = value.get("reference") {
            if let Some(s) = ref_value.as_str() {
                refs.push(s.to_string());
            }
            return;
        }

        // Array of references
        if let Some(arr) = value.as_array() {
            for item in arr {
                Self::collect_references(item, refs);
            }
        }
    }

    /// Get compartment-restricted search parameters.
    ///
    /// Returns a list of (parameter, value) pairs that can be used to filter
    /// search results to resources in the specified compartment.
    #[must_use]
    pub fn get_compartment_search_params(
        &self,
        compartment_type: &str,
        compartment_id: &str,
        resource_type: &str,
    ) -> Option<Vec<(String, String)>> {
        let definition = match compartment_type {
            "Patient" => &self.patient_compartment,
            "Practitioner" => &self.practitioner_compartment,
            _ => return None,
        };

        let inclusions = definition.get_inclusions(resource_type)?;

        let params: Vec<(String, String)> = inclusions
            .iter()
            .filter(|i| i.param != "{def}")
            .map(|i| {
                (
                    i.param.clone(),
                    format!("{}/{}", compartment_type, compartment_id),
                )
            })
            .collect();

        if params.is_empty() {
            None
        } else {
            Some(params)
        }
    }

    /// Check if a resource type is in the specified compartment definition.
    #[must_use]
    pub fn is_resource_in_compartment_definition(
        &self,
        compartment_type: &str,
        resource_type: &str,
    ) -> bool {
        match compartment_type {
            "Patient" => self
                .patient_compartment
                .includes_resource_type(resource_type),
            "Practitioner" => self
                .practitioner_compartment
                .includes_resource_type(resource_type),
            _ => false,
        }
    }
}

// =============================================================================
// Patient Compartment Policy
// =============================================================================

/// Policy that restricts Patient users to their own compartment.
///
/// This policy is designed for patient-facing applications where
/// patients should only access resources related to themselves.
pub struct PatientCompartmentPolicy {
    checker: CompartmentChecker,
}

impl Default for PatientCompartmentPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PatientCompartmentPolicy {
    /// Create a new patient compartment policy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checker: CompartmentChecker::new(),
        }
    }

    /// Evaluate compartment-based access.
    ///
    /// Returns:
    /// - `Allow` if access is granted
    /// - `Deny` if access violates compartment boundaries
    /// - `Abstain` if this policy doesn't apply
    #[must_use]
    pub fn evaluate(&self, context: &PolicyContext) -> AccessDecision {
        // Only applies to Patient users
        let (user_type, user_id) = match &context.user {
            Some(user) => (user.fhir_user_type.as_deref(), user.fhir_user_id.as_deref()),
            None => return AccessDecision::Abstain,
        };

        // Only for Patient users
        if user_type != Some("Patient") {
            return AccessDecision::Abstain;
        }

        let patient_id = match user_id {
            Some(id) => id,
            None => return AccessDecision::Abstain,
        };

        // Check if accessing Patient resource directly
        if context.request.resource_type == "Patient"
            && let Some(ref id) = context.request.resource_id
        {
            if id == patient_id {
                return AccessDecision::Allow;
            } else {
                return AccessDecision::Deny(DenyReason {
                    code: "compartment-violation".to_string(),
                    message: "Cannot access other patients' records".to_string(),
                    details: None,
                    policy_id: None,
                });
            }
        }

        // Check if resource is in patient's compartment
        if let Some(ref resource_ctx) = context.resource {
            if self
                .checker
                .is_in_compartment("Patient", patient_id, &resource_ctx.resource)
            {
                return AccessDecision::Allow;
            } else {
                return AccessDecision::Deny(DenyReason {
                    code: "compartment-violation".to_string(),
                    message: "Resource not in patient's compartment".to_string(),
                    details: None,
                    policy_id: None,
                });
            }
        }

        // For searches, check if resource type is in compartment definition
        if matches!(
            context.request.operation,
            FhirOperation::Search | FhirOperation::SearchType
        ) {
            if self
                .checker
                .is_resource_in_compartment_definition("Patient", &context.request.resource_type)
            {
                // Resource type is in compartment, allow search (filtering should be applied)
                return AccessDecision::Abstain;
            } else {
                // Resource type is not in patient compartment
                return AccessDecision::Deny(DenyReason {
                    code: "compartment-violation".to_string(),
                    message: format!(
                        "Resource type '{}' is not accessible to patients",
                        context.request.resource_type
                    ),
                    details: None,
                    policy_id: None,
                });
            }
        }

        AccessDecision::Abstain
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::context::{
        ClientIdentity, ClientType, EnvironmentContext, RequestContext, ResourceContext,
        ScopeSummary, UserIdentity,
    };
    use std::collections::HashMap;
    use time::OffsetDateTime;

    #[test]
    fn test_patient_compartment_definition() {
        let def = CompartmentDefinition::patient();

        assert!(def.includes_resource_type("Patient"));
        assert!(def.includes_resource_type("Observation"));
        assert!(def.includes_resource_type("Condition"));
        assert!(def.includes_resource_type("MedicationRequest"));
        assert!(!def.includes_resource_type("Organization"));
        assert!(!def.includes_resource_type("Medication"));
    }

    #[test]
    fn test_practitioner_compartment_definition() {
        let def = CompartmentDefinition::practitioner();

        assert!(def.includes_resource_type("Practitioner"));
        assert!(def.includes_resource_type("Encounter"));
        assert!(def.includes_resource_type("Observation"));
        assert!(!def.includes_resource_type("Patient"));
    }

    #[test]
    fn test_patient_self_in_compartment() {
        let checker = CompartmentChecker::new();

        let patient = serde_json::json!({
            "resourceType": "Patient",
            "id": "123"
        });

        assert!(checker.is_in_compartment("Patient", "123", &patient));
        assert!(!checker.is_in_compartment("Patient", "456", &patient));
    }

    #[test]
    fn test_observation_in_patient_compartment() {
        let checker = CompartmentChecker::new();

        let observation = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {
                "reference": "Patient/123"
            }
        });

        assert!(checker.is_in_compartment("Patient", "123", &observation));
        assert!(!checker.is_in_compartment("Patient", "456", &observation));
    }

    #[test]
    fn test_observation_not_in_compartment() {
        let checker = CompartmentChecker::new();

        let observation = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {
                "reference": "Patient/789"
            }
        });

        assert!(!checker.is_in_compartment("Patient", "123", &observation));
    }

    #[test]
    fn test_resource_not_in_any_compartment() {
        let checker = CompartmentChecker::new();

        let organization = serde_json::json!({
            "resourceType": "Organization",
            "id": "org-1"
        });

        assert!(!checker.is_in_compartment("Patient", "123", &organization));
    }

    #[test]
    fn test_array_performer() {
        let checker = CompartmentChecker::new();

        let observation = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "performer": [
                {"reference": "Practitioner/456"},
                {"reference": "Patient/123"}
            ]
        });

        // Patient as performer puts it in Patient compartment
        assert!(checker.is_in_compartment("Patient", "123", &observation));
    }

    #[test]
    fn test_condition_with_asserter() {
        let checker = CompartmentChecker::new();

        let condition = serde_json::json!({
            "resourceType": "Condition",
            "id": "cond-1",
            "subject": {"reference": "Patient/456"},
            "asserter": {"reference": "Patient/123"}
        });

        // Patient as asserter puts it in their compartment
        assert!(checker.is_in_compartment("Patient", "123", &condition));
        // Patient as subject also puts it in their compartment
        assert!(checker.is_in_compartment("Patient", "456", &condition));
    }

    #[test]
    fn test_allergy_with_patient_param() {
        let checker = CompartmentChecker::new();

        let allergy = serde_json::json!({
            "resourceType": "AllergyIntolerance",
            "id": "allergy-1",
            "patient": {"reference": "Patient/123"}
        });

        assert!(checker.is_in_compartment("Patient", "123", &allergy));
        assert!(!checker.is_in_compartment("Patient", "456", &allergy));
    }

    #[test]
    fn test_compartment_search_params() {
        let checker = CompartmentChecker::new();

        let params = checker.get_compartment_search_params("Patient", "123", "Observation");
        assert!(params.is_some());

        let params = params.unwrap();
        assert!(params.iter().any(|(k, _)| k == "subject"));
        assert!(params.iter().any(|(k, _)| k == "performer"));
    }

    #[test]
    fn test_compartment_search_params_patient_resource() {
        let checker = CompartmentChecker::new();

        // Patient resource uses {def} which should be filtered out
        let params = checker.get_compartment_search_params("Patient", "123", "Patient");
        assert!(params.is_none());
    }

    fn create_patient_context(
        patient_id: &str,
        resource_type: &str,
        resource_id: Option<&str>,
    ) -> PolicyContext {
        PolicyContext {
            user: Some(UserIdentity {
                id: format!("user-{}", patient_id),
                fhir_user: Some(format!("Patient/{}", patient_id)),
                fhir_user_type: Some("Patient".to_string()),
                fhir_user_id: Some(patient_id.to_string()),
                roles: vec![],
                attributes: HashMap::new(),
            }),
            client: ClientIdentity {
                id: "client-1".to_string(),
                name: "Test Client".to_string(),
                trusted: false,
                client_type: ClientType::ConfidentialSymmetric,
            },
            scopes: ScopeSummary {
                raw: "patient/*.read".to_string(),
                patient_scopes: vec!["patient/*.read".to_string()],
                user_scopes: vec![],
                system_scopes: vec![],
                has_wildcard: true,
                launch: false,
                openid: false,
                fhir_user: false,
                offline_access: false,
            },
            request: RequestContext {
                operation: FhirOperation::Read,
                resource_type: resource_type.to_string(),
                resource_id: resource_id.map(String::from),
                compartment_type: None,
                compartment_id: None,
                body: None,
                query_params: HashMap::new(),
                path: format!("/{}/{}", resource_type, resource_id.unwrap_or("")),
                method: "GET".to_string(),
                operation_id: None,
            },
            resource: None,
            environment: EnvironmentContext {
                request_time: OffsetDateTime::now_utc(),
                source_ip: None,
                request_id: "req-1".to_string(),
                patient_context: Some(patient_id.to_string()),
                encounter_context: None,
            },
        }
    }

    #[test]
    fn test_patient_compartment_policy_own_patient() {
        let policy = PatientCompartmentPolicy::new();

        // Patient accessing own Patient record
        let context = create_patient_context("123", "Patient", Some("123"));
        let decision = policy.evaluate(&context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_patient_compartment_policy_other_patient() {
        let policy = PatientCompartmentPolicy::new();

        // Patient accessing other Patient's record
        let context = create_patient_context("123", "Patient", Some("456"));
        let decision = policy.evaluate(&context);
        assert!(decision.is_denied());

        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.code, "compartment-violation");
        }
    }

    #[test]
    fn test_patient_accessing_own_observation() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Observation", Some("obs-1"));
        context.resource = Some(ResourceContext {
            id: "obs-1".to_string(),
            resource_type: "Observation".to_string(),
            version_id: None,
            last_updated: None,
            subject: Some("Patient/123".to_string()),
            author: None,
            resource: serde_json::json!({
                "resourceType": "Observation",
                "id": "obs-1",
                "subject": {"reference": "Patient/123"}
            }),
        });

        let decision = policy.evaluate(&context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_patient_accessing_other_observation() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Observation", Some("obs-2"));
        context.resource = Some(ResourceContext {
            id: "obs-2".to_string(),
            resource_type: "Observation".to_string(),
            version_id: None,
            last_updated: None,
            subject: Some("Patient/456".to_string()),
            author: None,
            resource: serde_json::json!({
                "resourceType": "Observation",
                "id": "obs-2",
                "subject": {"reference": "Patient/456"}
            }),
        });

        let decision = policy.evaluate(&context);
        assert!(decision.is_denied());
    }

    #[test]
    fn test_patient_search_allowed_resource_type() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Observation", None);
        context.request.operation = FhirOperation::Search;
        context.request.resource_id = None;

        let decision = policy.evaluate(&context);
        // Abstain for searches - filtering should be applied elsewhere
        assert!(decision.is_abstain());
    }

    #[test]
    fn test_patient_search_disallowed_resource_type() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Organization", None);
        context.request.operation = FhirOperation::Search;
        context.request.resource_id = None;

        let decision = policy.evaluate(&context);
        // Deny for resource types not in patient compartment
        assert!(decision.is_denied());
    }

    #[test]
    fn test_practitioner_user_abstains() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Patient", Some("456"));
        context.user = Some(UserIdentity {
            id: "prac-1".to_string(),
            fhir_user: Some("Practitioner/789".to_string()),
            fhir_user_type: Some("Practitioner".to_string()),
            fhir_user_id: Some("789".to_string()),
            roles: vec!["doctor".to_string()],
            attributes: HashMap::new(),
        });

        let decision = policy.evaluate(&context);
        // Abstain for non-Patient users
        assert!(decision.is_abstain());
    }

    #[test]
    fn test_no_user_abstains() {
        let policy = PatientCompartmentPolicy::new();

        let mut context = create_patient_context("123", "Patient", Some("123"));
        context.user = None;

        let decision = policy.evaluate(&context);
        assert!(decision.is_abstain());
    }

    #[test]
    fn test_encounter_with_patient() {
        let checker = CompartmentChecker::new();

        let encounter = serde_json::json!({
            "resourceType": "Encounter",
            "id": "enc-1",
            "patient": {"reference": "Patient/123"}
        });

        // Note: Encounter uses "patient" param, not "subject"
        assert!(checker.is_in_compartment("Patient", "123", &encounter));
    }

    #[test]
    fn test_medication_request_with_subject() {
        let checker = CompartmentChecker::new();

        let med_request = serde_json::json!({
            "resourceType": "MedicationRequest",
            "id": "med-1",
            "subject": {"reference": "Patient/123"}
        });

        assert!(checker.is_in_compartment("Patient", "123", &med_request));
    }

    #[test]
    fn test_practitioner_self_in_compartment() {
        let checker = CompartmentChecker::new();

        let practitioner = serde_json::json!({
            "resourceType": "Practitioner",
            "id": "prac-1"
        });

        assert!(checker.is_in_compartment("Practitioner", "prac-1", &practitioner));
        assert!(!checker.is_in_compartment("Practitioner", "prac-2", &practitioner));
    }

    #[test]
    fn test_observation_in_practitioner_compartment() {
        let checker = CompartmentChecker::new();

        let observation = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "performer": [
                {"reference": "Practitioner/prac-1"}
            ]
        });

        assert!(checker.is_in_compartment("Practitioner", "prac-1", &observation));
        assert!(!checker.is_in_compartment("Practitioner", "prac-2", &observation));
    }
}
