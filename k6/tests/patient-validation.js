// Patient Validation Tests - Testing rejection of invalid resources
// Run: k6 run k6/tests/patient-validation.js

import { check, group } from "k6";
import { Counter } from "k6/metrics";
import { config, thresholds, scenarios } from "../lib/config.js";
import {
  createResource,
  updateResource,
  checkValidationError,
  searchResources,
  extractResourceId,
  deleteResource,
  generateTestId,
} from "../lib/fhir.js";
import { invalidPatients, validPatients } from "../data/patients.js";

// Custom metrics
const validationErrorsCaught = new Counter("validation_errors_caught");
const unexpectedSuccesses = new Counter("unexpected_successes");

// Test configuration
export const options = {
  scenarios: {
    functional: scenarios.functional,
  },
  thresholds: {
    ...thresholds.functional,
    validation_errors_caught: ["count>10"], // Should catch many validation errors
  },
};

// Main test function
export default function () {
  const testId = generateTestId();
  let createdIds = []; // Track any accidentally created resources for cleanup

  // ==================== CREATE VALIDATION TESTS ====================
  group("CREATE Validation - Invalid Resources", function () {
    // Test: Missing resourceType
    group("Missing resourceType", function () {
      const response = createResource("Patient", invalidPatients.missingResourceType);

      if (checkValidationError(response, "Missing resourceType")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient without resourceType");
      }
    });

    // Test: Wrong resourceType
    group("Wrong resourceType", function () {
      const response = createResource("Patient", invalidPatients.wrongResourceType);

      if (checkValidationError(response, "Wrong resourceType")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted resource with wrong resourceType");
      }
    });

    // Test: Invalid gender code
    group("Invalid gender code", function () {
      const response = createResource("Patient", invalidPatients.invalidGender);

      if (checkValidationError(response, "Invalid gender")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid gender code");
      }
    });

    // Test: Invalid birthDate format
    group("Invalid birthDate format", function () {
      const response = createResource("Patient", invalidPatients.invalidBirthDate);

      if (checkValidationError(response, "Invalid birthDate")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid birthDate");
      }
    });

    // Test: Invalid telecom system
    group("Invalid telecom system", function () {
      const response = createResource("Patient", invalidPatients.invalidTelecomSystem);

      if (checkValidationError(response, "Invalid telecom system")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid telecom system");
      }
    });

    // Test: Invalid address use
    group("Invalid address use", function () {
      const response = createResource("Patient", invalidPatients.invalidAddressUse);

      if (checkValidationError(response, "Invalid address use")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid address use");
      }
    });

    // Test: Empty object
    group("Empty object", function () {
      const response = createResource("Patient", invalidPatients.emptyObject);

      if (checkValidationError(response, "Empty object")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted empty object as Patient");
      }
    });

    // Test: Null value
    group("Null value", function () {
      const response = createResource("Patient", invalidPatients.nullValue);

      if (checkValidationError(response, "Null value")) {
        validationErrorsCaught.add(1);
      } else {
        check(response, {
          "Null rejected with error status": (r) =>
            r.status >= 400 && r.status < 500,
        });
      }
    });

    // Test: Array instead of object
    group("Array instead of object", function () {
      const response = createResource(
        "Patient",
        invalidPatients.arrayInsteadOfObject
      );

      if (checkValidationError(response, "Array instead of object")) {
        validationErrorsCaught.add(1);
      } else {
        check(response, {
          "Array rejected with error status": (r) =>
            r.status >= 400 && r.status < 500,
        });
      }
    });

    // Test: String instead of object
    group("String instead of object", function () {
      const response = createResource(
        "Patient",
        invalidPatients.stringInsteadOfObject
      );

      if (checkValidationError(response, "String instead of object")) {
        validationErrorsCaught.add(1);
      } else {
        check(response, {
          "String rejected with error status": (r) =>
            r.status >= 400 && r.status < 500,
        });
      }
    });

    // Test: Invalid nested structure (name as string instead of array)
    group("Invalid nested structure", function () {
      const response = createResource(
        "Patient",
        invalidPatients.invalidNestedStructure
      );

      if (checkValidationError(response, "Invalid nested structure")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid nested structure");
      }
    });

    // Test: Invalid identifier (object instead of array)
    group("Invalid identifier structure", function () {
      const response = createResource("Patient", invalidPatients.invalidIdentifier);

      if (checkValidationError(response, "Invalid identifier")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid identifier structure");
      }
    });

    // Test: Conflicting deceased values (both boolean and dateTime)
    group("Conflicting deceased values", function () {
      const response = createResource("Patient", invalidPatients.conflictingDeceased);

      if (checkValidationError(response, "Conflicting deceased")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn(
          "Server accepted Patient with conflicting deceased[x] values"
        );
      }
    });

    // Test: Conflicting multipleBirth values
    group("Conflicting multipleBirth values", function () {
      const response = createResource(
        "Patient",
        invalidPatients.conflictingMultipleBirth
      );

      if (checkValidationError(response, "Conflicting multipleBirth")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn(
          "Server accepted Patient with conflicting multipleBirth[x] values"
        );
      }
    });

    // Test: Invalid reference format
    group("Invalid reference format", function () {
      const response = createResource("Patient", invalidPatients.invalidReference);

      if (checkValidationError(response, "Invalid reference")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid reference format");
      }
    });

    // Test: Unknown fields
    group("Unknown fields", function () {
      const response = createResource("Patient", invalidPatients.unknownFields);

      // Note: Some servers accept unknown fields, others reject them
      check(response, {
        "Unknown fields handled (200, 201, 400, or 422)": (r) =>
          r.status === 200 ||
          r.status === 201 ||
          r.status === 400 ||
          r.status === 422,
      });

      if (response.status === 400 || response.status === 422) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.log(
          "Server accepts unknown fields (this is valid per FHIR spec for open types)"
        );
      }
    });

    // Test: Invalid contact structure
    group("Invalid contact structure", function () {
      const response = createResource("Patient", invalidPatients.invalidContact);

      if (checkValidationError(response, "Invalid contact")) {
        validationErrorsCaught.add(1);
      } else if (response.status === 201) {
        unexpectedSuccesses.add(1);
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
        console.warn("Server accepted Patient with invalid contact structure");
      }
    });
  });

  // ==================== UPDATE VALIDATION TESTS ====================
  group("UPDATE Validation - Invalid Resources", function () {
    // First create a valid Patient for update tests
    let testPatientId = null;
    const validPatient = {
      ...validPatients[0],
      identifier: [
        {
          system: `http://test.example.org/${testId}`,
          value: "validation-test-patient",
        },
      ],
    };

    const createResponse = createResource("Patient", validPatient);
    if (createResponse.status === 201) {
      testPatientId = extractResourceId(createResponse);
      createdIds.push(testPatientId);
    } else {
      console.warn("Could not create test patient for update validation tests");
      return;
    }

    // Test: Update with wrong resourceType
    group("Update with wrong resourceType", function () {
      const invalidUpdate = {
        resourceType: "Observation",
        id: testPatientId,
        status: "final",
      };
      const response = updateResource("Patient", testPatientId, invalidUpdate);

      if (checkValidationError(response, "Wrong resourceType in update")) {
        validationErrorsCaught.add(1);
      }
    });

    // Test: Update with mismatched ID
    group("Update with mismatched ID", function () {
      const invalidUpdate = {
        resourceType: "Patient",
        id: "different-id",
        name: [{ family: "Test" }],
      };
      const response = updateResource("Patient", testPatientId, invalidUpdate);

      // Should either reject (400) or use URL id (200)
      check(response, {
        "Mismatched ID handled": (r) =>
          r.status === 200 || r.status === 400 || r.status === 422,
      });
    });

    // Test: Update with invalid gender
    group("Update with invalid gender", function () {
      const invalidUpdate = {
        resourceType: "Patient",
        id: testPatientId,
        gender: "not-a-valid-gender",
      };
      const response = updateResource("Patient", testPatientId, invalidUpdate);

      if (checkValidationError(response, "Invalid gender in update")) {
        validationErrorsCaught.add(1);
      }
    });

    // Test: Update with invalid date
    group("Update with invalid birthDate", function () {
      const invalidUpdate = {
        resourceType: "Patient",
        id: testPatientId,
        birthDate: "invalid-date-format",
      };
      const response = updateResource("Patient", testPatientId, invalidUpdate);

      if (checkValidationError(response, "Invalid birthDate in update")) {
        validationErrorsCaught.add(1);
      }
    });
  });

  // ==================== CONTENT TYPE TESTS ====================
  group("Content Type Validation", function () {
    const validPatient = { resourceType: "Patient" };

    // Test: Wrong content type
    group("Wrong content type (text/plain)", function () {
      const response = createResource("Patient", "not json");

      check(response, {
        "Wrong content type rejected": (r) =>
          r.status === 400 || r.status === 415,
      });
    });
  });

  // ==================== MALFORMED JSON TESTS ====================
  group("Malformed JSON Tests", function () {
    // Note: k6's http.post with object will serialize to JSON
    // To test malformed JSON, we need to send raw string

    // Test: Truncated JSON (simulated by sending partial data)
    group("Incomplete JSON structure", function () {
      // This will be serialized as valid JSON, so we test structure issues instead
      const incomplete = {
        resourceType: "Patient",
        name: [
          {
            // Missing required closing structure in real scenario
          },
        ],
      };
      const response = createResource("Patient", incomplete);

      // Empty name object might be accepted or rejected depending on validation level
      check(response, {
        "Incomplete structure handled": (r) =>
          r.status === 200 ||
          r.status === 201 ||
          r.status === 400 ||
          r.status === 422,
      });
    });
  });

  // ==================== EDGE CASES ====================
  group("Edge Cases", function () {
    // Test: Very long string values
    group("Very long name", function () {
      const longName = "A".repeat(10000);
      const patient = {
        resourceType: "Patient",
        name: [
          {
            family: longName,
          },
        ],
      };
      const response = createResource("Patient", patient);

      // Server might accept or reject based on limits
      check(response, {
        "Long name handled gracefully": (r) => r.status < 500,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test: Special characters in strings
    group("Special characters in name", function () {
      const patient = {
        resourceType: "Patient",
        name: [
          {
            family: "O'Brien-Smith<script>alert('xss')</script>",
            given: ["José", "François", "北京"],
          },
        ],
      };
      const response = createResource("Patient", patient);

      // Should accept unicode, handle special chars safely
      check(response, {
        "Special characters handled": (r) =>
          r.status === 200 || r.status === 201 || r.status === 400,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test: Deeply nested extensions
    group("Deeply nested structure", function () {
      const patient = {
        resourceType: "Patient",
        extension: [
          {
            url: "http://example.org/ext1",
            extension: [
              {
                url: "http://example.org/ext2",
                extension: [
                  {
                    url: "http://example.org/ext3",
                    valueString: "deeply nested",
                  },
                ],
              },
            ],
          },
        ],
      };
      const response = createResource("Patient", patient);

      check(response, {
        "Deeply nested handled": (r) =>
          r.status === 200 || r.status === 201 || r.status === 400,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test: Empty arrays
    group("Empty arrays", function () {
      const patient = {
        resourceType: "Patient",
        name: [],
        telecom: [],
        address: [],
      };
      const response = createResource("Patient", patient);

      // FHIR spec says arrays should not be empty if present
      check(response, {
        "Empty arrays handled": (r) =>
          r.status === 200 ||
          r.status === 201 ||
          r.status === 400 ||
          r.status === 422,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test: Future birthDate
    group("Future birthDate", function () {
      const futureDate = new Date();
      futureDate.setFullYear(futureDate.getFullYear() + 10);
      const patient = {
        resourceType: "Patient",
        birthDate: futureDate.toISOString().split("T")[0],
      };
      const response = createResource("Patient", patient);

      // FHIR doesn't technically prohibit future dates, but server might
      check(response, {
        "Future birthDate handled": (r) =>
          r.status === 200 ||
          r.status === 201 ||
          r.status === 400 ||
          r.status === 422,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test: Very old birthDate
    group("Very old birthDate", function () {
      const patient = {
        resourceType: "Patient",
        birthDate: "1800-01-01",
      };
      const response = createResource("Patient", patient);

      check(response, {
        "Very old birthDate handled": (r) =>
          r.status === 200 ||
          r.status === 201 ||
          r.status === 400 ||
          r.status === 422,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });
  });

  // ==================== CLEANUP ====================
  group("Cleanup", function () {
    for (const id of createdIds) {
      deleteResource("Patient", id);
    }
  });
}

// Setup
export function setup() {
  console.log(`Validation tests against: ${config.baseUrl}`);

  const response = searchResources("Patient", { _count: 1 });
  if (response.status !== 200) {
    console.error(`Server not reachable. Status: ${response.status}`);
  }

  return { startTime: Date.now() };
}

// Teardown
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nValidation tests completed in ${duration.toFixed(2)}s`);
}
