/**
 * Client-side FHIR resource template generator.
 * Provides minimal valid templates for common FHIR resources.
 */

/**
 * Hardcoded templates for common FHIR R4B resources.
 * Each template includes the minimal required fields.
 */
const FHIR_TEMPLATES: Record<string, object> = {
	Patient: {
		resourceType: "Patient",
		name: [
			{
				family: "",
				given: [""],
			},
		],
		gender: "unknown",
		birthDate: "",
	},
	Observation: {
		resourceType: "Observation",
		status: "final",
		code: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		subject: {
			reference: "",
		},
	},
	Condition: {
		resourceType: "Condition",
		clinicalStatus: {
			coding: [
				{
					system: "http://terminology.hl7.org/CodeSystem/condition-clinical",
					code: "active",
				},
			],
		},
		code: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		subject: {
			reference: "",
		},
	},
	Encounter: {
		resourceType: "Encounter",
		status: "in-progress",
		class: {
			system: "http://terminology.hl7.org/CodeSystem/v3-ActCode",
			code: "AMB",
			display: "ambulatory",
		},
		subject: {
			reference: "",
		},
	},
	Procedure: {
		resourceType: "Procedure",
		status: "completed",
		code: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		subject: {
			reference: "",
		},
	},
	MedicationRequest: {
		resourceType: "MedicationRequest",
		status: "active",
		intent: "order",
		medicationCodeableConcept: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		subject: {
			reference: "",
		},
	},
	DiagnosticReport: {
		resourceType: "DiagnosticReport",
		status: "final",
		code: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		subject: {
			reference: "",
		},
	},
	Practitioner: {
		resourceType: "Practitioner",
		name: [
			{
				family: "",
				given: [""],
			},
		],
	},
	Organization: {
		resourceType: "Organization",
		name: "",
	},
	Location: {
		resourceType: "Location",
		status: "active",
		name: "",
	},
	Appointment: {
		resourceType: "Appointment",
		status: "proposed",
		participant: [
			{
				actor: {
					reference: "",
				},
				status: "needs-action",
			},
		],
	},
	AllergyIntolerance: {
		resourceType: "AllergyIntolerance",
		clinicalStatus: {
			coding: [
				{
					system: "http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical",
					code: "active",
				},
			],
		},
		code: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		patient: {
			reference: "",
		},
	},
	Immunization: {
		resourceType: "Immunization",
		status: "completed",
		vaccineCode: {
			coding: [
				{
					system: "",
					code: "",
					display: "",
				},
			],
		},
		patient: {
			reference: "",
		},
		occurrenceDateTime: "",
	},
	CarePlan: {
		resourceType: "CarePlan",
		status: "draft",
		intent: "plan",
		subject: {
			reference: "",
		},
	},
	Claim: {
		resourceType: "Claim",
		status: "active",
		type: {
			coding: [
				{
					system: "",
					code: "",
				},
			],
		},
		use: "claim",
		patient: {
			reference: "",
		},
		created: "",
		provider: {
			reference: "",
		},
		priority: {
			coding: [
				{
					code: "normal",
				},
			],
		},
		insurance: [
			{
				sequence: 1,
				focal: true,
				coverage: {
					reference: "",
				},
			},
		],
	},
};

/**
 * Generate a FHIR resource template for the given resource type.
 * Returns a hardcoded template if available, otherwise a minimal fallback.
 *
 * @param resourceType - FHIR resource type (e.g., "Patient", "Observation")
 * @returns JSON string of the template resource
 *
 * @example
 * generateTemplate("Patient")
 * // Returns formatted JSON string with Patient template
 *
 * generateTemplate("CustomResource")
 * // Returns minimal fallback: { "resourceType": "CustomResource" }
 */
export function generateTemplate(resourceType: string): string {
	const template = FHIR_TEMPLATES[resourceType];

	if (template) {
		// Return formatted template
		return JSON.stringify(template, null, 2);
	}

	// Fallback: minimal template with just resourceType
	const fallback = {
		resourceType,
	};

	return JSON.stringify(fallback, null, 2);
}

/**
 * Check if a resource type has a predefined template.
 *
 * @param resourceType - FHIR resource type
 * @returns true if template exists
 */
export function hasTemplate(resourceType: string): boolean {
	return resourceType in FHIR_TEMPLATES;
}

/**
 * Get list of all resource types with predefined templates.
 *
 * @returns Array of resource type names
 */
export function getTemplateResourceTypes(): string[] {
	return Object.keys(FHIR_TEMPLATES).sort();
}
