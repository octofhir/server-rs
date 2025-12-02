// FHIR resource fixtures for k6 tests

export const fixtures = {
  patient: {
    resourceType: "Patient",
    name: [
      {
        family: "Doe",
        given: ["John"],
      },
    ],
    gender: "male",
    birthDate: "1990-01-01",
    identifier: [
      {
        system: "http://example.org/mrn",
        value: "MRN-" + Math.floor(Math.random() * 1000000),
      },
    ],
  },

  observation: {
    resourceType: "Observation",
    status: "final",
    code: {
      coding: [
        {
          system: "http://loinc.org",
          code: "15074-8",
          display: "Glucose [Moles/volume] in Blood",
        },
      ],
    },
    subject: {
      reference: "Patient/example",
    },
    effectiveDateTime: new Date().toISOString(),
    valueQuantity: {
      value: 6.3,
      unit: "mmol/l",
      system: "http://unitsofmeasure.org",
      code: "mmol/L",
    },
  },

  condition: {
    resourceType: "Condition",
    clinicalStatus: {
      coding: [
        {
          system:
            "http://terminology.hl7.org/CodeSystem/condition-clinical",
          code: "active",
        },
      ],
    },
    verificationStatus: {
      coding: [
        {
          system:
            "http://terminology.hl7.org/CodeSystem/condition-ver-status",
          code: "confirmed",
        },
      ],
    },
    code: {
      coding: [
        {
          system: "http://snomed.info/sct",
          code: "44054006",
          display: "Type 2 diabetes mellitus",
        },
      ],
    },
    subject: {
      reference: "Patient/example",
    },
    onsetDateTime: "2020-01-01",
  },

  medicationRequest: {
    resourceType: "MedicationRequest",
    status: "active",
    intent: "order",
    medicationCodeableConcept: {
      coding: [
        {
          system: "http://www.nlm.nih.gov/research/umls/rxnorm",
          code: "860975",
          display: "metformin hydrochloride 500 MG Oral Tablet",
        },
      ],
    },
    subject: {
      reference: "Patient/example",
    },
    authoredOn: new Date().toISOString(),
    dosageInstruction: [
      {
        text: "Take 1 tablet twice daily",
        timing: {
          repeat: {
            frequency: 2,
            period: 1,
            periodUnit: "d",
          },
        },
      },
    ],
  },

  encounter: {
    resourceType: "Encounter",
    status: "finished",
    class: {
      system: "http://terminology.hl7.org/CodeSystem/v3-ActCode",
      code: "AMB",
      display: "ambulatory",
    },
    subject: {
      reference: "Patient/example",
    },
    period: {
      start: "2023-01-01T10:00:00Z",
      end: "2023-01-01T11:00:00Z",
    },
  },

  allergyIntolerance: {
    resourceType: "AllergyIntolerance",
    clinicalStatus: {
      coding: [
        {
          system:
            "http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical",
          code: "active",
        },
      ],
    },
    verificationStatus: {
      coding: [
        {
          system:
            "http://terminology.hl7.org/CodeSystem/allergyintolerance-verification",
          code: "confirmed",
        },
      ],
    },
    code: {
      coding: [
        {
          system: "http://snomed.info/sct",
          code: "227037002",
          display: "Fish - dietary",
        },
      ],
    },
    patient: {
      reference: "Patient/example",
    },
  },

  procedure: {
    resourceType: "Procedure",
    status: "completed",
    code: {
      coding: [
        {
          system: "http://snomed.info/sct",
          code: "80146002",
          display: "Appendectomy",
        },
      ],
    },
    subject: {
      reference: "Patient/example",
    },
    performedDateTime: "2022-06-15",
  },

  bundle: {
    resourceType: "Bundle",
    type: "transaction",
    entry: [
      {
        request: {
          method: "POST",
          url: "Patient",
        },
        resource: null, // Will be filled with actual resource
      },
      {
        request: {
          method: "POST",
          url: "Observation",
        },
        resource: null, // Will be filled with actual resource
      },
    ],
  },
};

// Create a copy of a fixture with updated reference
export function withSubject(fixture, patientId) {
  const copy = JSON.parse(JSON.stringify(fixture));
  if (copy.subject) {
    copy.subject.reference = `Patient/${patientId}`;
  }
  if (copy.patient) {
    copy.patient.reference = `Patient/${patientId}`;
  }
  return copy;
}

// Create a transaction bundle with the given entries
export function createTransactionBundle(entries) {
  return {
    resourceType: "Bundle",
    type: "transaction",
    entry: entries,
  };
}

// Generate random test data
export function randomString(length = 10) {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let result = "";
  for (let i = 0; i < length; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result;
}

export function randomDate(start = new Date(2020, 0, 1), end = new Date()) {
  const date = new Date(
    start.getTime() + Math.random() * (end.getTime() - start.getTime())
  );
  return date.toISOString().split("T")[0];
}

export function randomValue(min = 0, max = 100) {
  return Math.random() * (max - min) + min;
}
