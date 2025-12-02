// Patient CRUD Functional Tests
// Run: k6 run k6/tests/patient-crud.js

import { check, group, fail } from "k6";
import { Counter, Trend } from "k6/metrics";
import { config, thresholds, scenarios } from "../lib/config.js";
import {
  createResource,
  readResource,
  updateResource,
  deleteResource,
  searchResources,
  readHistory,
  vreadResource,
  extractResourceId,
  extractVersionId,
  checkCreated,
  checkRead,
  checkUpdated,
  checkDeleted,
  checkNotFound,
  checkValidationError,
  checkSearchBundle,
  generateTestId,
} from "../lib/fhir.js";
import {
  validPatients,
  minimalPatient,
  fullPatient,
  invalidPatients,
  patientUpdates,
  generateRandomPatient,
} from "../data/patients.js";

// Custom metrics
const createDuration = new Trend("patient_create_duration");
const readDuration = new Trend("patient_read_duration");
const updateDuration = new Trend("patient_update_duration");
const deleteDuration = new Trend("patient_delete_duration");
const searchDuration = new Trend("patient_search_duration");
const validationErrors = new Counter("validation_errors_caught");
const crudCycles = new Counter("crud_cycles_completed");

// Test configuration
export const options = {
  scenarios: {
    functional: scenarios.functional,
  },
  thresholds: thresholds.functional,
};

// Main test function
export default function () {
  const testId = generateTestId();
  let createdIds = [];

  // ==================== CREATE TESTS ====================
  group("CREATE Operations", function () {
    // Test 1: Create minimal Patient
    group("Create minimal Patient", function () {
      const start = Date.now();
      const response = createResource("Patient", minimalPatient);
      createDuration.add(Date.now() - start);

      if (checkCreated(response, "Minimal Patient")) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });

    // Test 2: Create full Patient with all fields
    group("Create full Patient", function () {
      const start = Date.now();
      const response = createResource("Patient", fullPatient);
      createDuration.add(Date.now() - start);

      if (checkCreated(response, "Full Patient")) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);

        // Verify all fields are stored correctly
        const readResponse = readResource("Patient", id);
        const body = JSON.parse(readResponse.body);
        check(body, {
          "Full Patient has identifier": (b) =>
            b.identifier && b.identifier.length > 0,
          "Full Patient has name": (b) => b.name && b.name.length > 0,
          "Full Patient has telecom": (b) =>
            b.telecom && b.telecom.length > 0,
          "Full Patient has address": (b) =>
            b.address && b.address.length > 0,
          "Full Patient has meta": (b) => b.meta && b.meta.versionId,
        });
      }
    });

    // Test 3: Create multiple valid Patients
    group("Create multiple valid Patients", function () {
      for (let i = 0; i < validPatients.length; i++) {
        const patient = {
          ...validPatients[i],
          identifier: [
            {
              system: `http://test.example.org/${testId}`,
              value: `patient-${i}`,
            },
          ],
        };

        const start = Date.now();
        const response = createResource("Patient", patient);
        createDuration.add(Date.now() - start);

        if (checkCreated(response, `Valid Patient ${i + 1}`)) {
          const id = extractResourceId(response);
          if (id) createdIds.push(id);
        }
      }
    });

    // Test 4: Create Patient with random data
    group("Create random Patient", function () {
      const randomPatient = generateRandomPatient();
      const start = Date.now();
      const response = createResource("Patient", randomPatient);
      createDuration.add(Date.now() - start);

      if (checkCreated(response, "Random Patient")) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });
  });

  // ==================== READ TESTS ====================
  group("READ Operations", function () {
    if (createdIds.length === 0) {
      console.warn("No patients created, skipping read tests");
      return;
    }

    // Test 1: Read existing Patient
    group("Read existing Patient", function () {
      const id = createdIds[0];
      const start = Date.now();
      const response = readResource("Patient", id);
      readDuration.add(Date.now() - start);

      checkRead(response, "Patient");

      // Verify response structure
      const body = JSON.parse(response.body);
      check(body, {
        "Patient has id": (b) => b.id === id,
        "Patient has meta.versionId": (b) => b.meta && b.meta.versionId,
        "Patient has meta.lastUpdated": (b) => b.meta && b.meta.lastUpdated,
      });
    });

    // Test 2: Read non-existent Patient
    group("Read non-existent Patient", function () {
      const start = Date.now();
      const response = readResource("Patient", "non-existent-id-12345");
      readDuration.add(Date.now() - start);

      checkNotFound(response, "Non-existent Patient");
    });

    // Test 3: Read all created Patients
    group("Read all created Patients", function () {
      for (const id of createdIds) {
        const start = Date.now();
        const response = readResource("Patient", id);
        readDuration.add(Date.now() - start);
        checkRead(response, `Patient ${id}`);
      }
    });
  });

  // ==================== UPDATE TESTS ====================
  group("UPDATE Operations", function () {
    if (createdIds.length === 0) {
      console.warn("No patients created, skipping update tests");
      return;
    }

    const testPatientId = createdIds[0];

    // Read current state
    let currentPatient;
    const readResponse = readResource("Patient", testPatientId);
    if (readResponse.status === 200) {
      currentPatient = JSON.parse(readResponse.body);
    } else {
      console.warn("Could not read patient for update tests");
      return;
    }

    // Test 1: Update name
    group("Update Patient name", function () {
      const updated = patientUpdates.nameUpdate(currentPatient);
      const start = Date.now();
      const response = updateResource("Patient", testPatientId, updated);
      updateDuration.add(Date.now() - start);

      if (checkUpdated(response, "Patient (name update)")) {
        const body = JSON.parse(response.body);
        check(body, {
          "Name was updated": (b) =>
            b.name &&
            b.name[0] &&
            b.name[0].family === "UpdatedFamily",
          "Version incremented": (b) =>
            parseInt(b.meta.versionId) >
            parseInt(currentPatient.meta.versionId),
        });
        currentPatient = body;
      }
    });

    // Test 2: Add telecom
    group("Add Patient telecom", function () {
      const updated = patientUpdates.addTelecom(currentPatient);
      const start = Date.now();
      const response = updateResource("Patient", testPatientId, updated);
      updateDuration.add(Date.now() - start);

      if (checkUpdated(response, "Patient (add telecom)")) {
        const body = JSON.parse(response.body);
        check(body, {
          "Telecom was added": (b) =>
            b.telecom &&
            b.telecom.some((t) => t.value === "+1-555-NEW-PHONE"),
        });
        currentPatient = body;
      }
    });

    // Test 3: Update address
    group("Update Patient address", function () {
      const updated = patientUpdates.addressUpdate(currentPatient);
      const start = Date.now();
      const response = updateResource("Patient", testPatientId, updated);
      updateDuration.add(Date.now() - start);

      if (checkUpdated(response, "Patient (address update)")) {
        const body = JSON.parse(response.body);
        check(body, {
          "Address was updated": (b) =>
            b.address && b.address[0] && b.address[0].city === "New City",
        });
        currentPatient = body;
      }
    });

    // Test 4: Deactivate Patient
    group("Deactivate Patient", function () {
      const updated = patientUpdates.deactivate(currentPatient);
      const start = Date.now();
      const response = updateResource("Patient", testPatientId, updated);
      updateDuration.add(Date.now() - start);

      if (checkUpdated(response, "Patient (deactivate)")) {
        const body = JSON.parse(response.body);
        check(body, {
          "Patient is inactive": (b) => b.active === false,
        });
      }
    });

    // Test 5: Update non-existent Patient (should create or fail)
    group("Update non-existent Patient", function () {
      const newId = `new-patient-${testId}`;
      const newPatient = {
        ...minimalPatient,
        id: newId,
      };
      const start = Date.now();
      const response = updateResource("Patient", newId, newPatient);
      updateDuration.add(Date.now() - start);

      // Server may return 201 (created) or 404/400 (depending on configuration)
      check(response, {
        "Update non-existent returns valid status": (r) =>
          r.status === 201 || r.status === 404 || r.status === 400,
      });

      if (response.status === 201) {
        const id = extractResourceId(response);
        if (id) createdIds.push(id);
      }
    });
  });

  // ==================== SEARCH TESTS ====================
  group("SEARCH Operations", function () {
    // Test 1: Search all Patients
    group("Search all Patients", function () {
      const start = Date.now();
      const response = searchResources("Patient");
      searchDuration.add(Date.now() - start);

      checkSearchBundle(response, "All Patients");
    });

    // Test 2: Search by name
    group("Search by family name", function () {
      const start = Date.now();
      const response = searchResources("Patient", { family: "UpdatedFamily" });
      searchDuration.add(Date.now() - start);

      checkSearchBundle(response, "By family name");
    });

    // Test 3: Search by gender (tests server search param implementation)
    group("Search by gender", function () {
      const start = Date.now();
      const response = searchResources("Patient", { gender: "male" });
      searchDuration.add(Date.now() - start);

      checkSearchBundle(response, "By gender");
    });

    // Test 4: Search with pagination
    group("Search with pagination", function () {
      const start = Date.now();
      const response = searchResources("Patient", { _count: 2 });
      searchDuration.add(Date.now() - start);

      if (checkSearchBundle(response, "With pagination")) {
        const body = JSON.parse(response.body);
        check(body, {
          "Respects _count limit": (b) =>
            !b.entry || b.entry.length <= 2,
          "Has link for navigation": (b) => b.link && b.link.length > 0,
        });
      }
    });

    // Test 5: Search by identifier
    group("Search by identifier", function () {
      const start = Date.now();
      const response = searchResources("Patient", {
        identifier: `http://test.example.org/${testId}|patient-0`,
      });
      searchDuration.add(Date.now() - start);

      checkSearchBundle(response, "By identifier");
    });

    // Test 6: Search with multiple parameters (tests combined search)
    group("Search with multiple params", function () {
      const start = Date.now();
      const response = searchResources("Patient", {
        gender: "female",
        _count: 5,
      });
      searchDuration.add(Date.now() - start);

      checkSearchBundle(response, "Multiple params");
    });

    // Test 7: Search with invalid parameter (should be ignored or return error)
    group("Search with invalid parameter", function () {
      const start = Date.now();
      const response = searchResources("Patient", {
        invalidParam: "value",
      });
      searchDuration.add(Date.now() - start);

      // Server might ignore unknown params or return an error
      check(response, {
        "Handles invalid param gracefully": (r) =>
          r.status === 200 || r.status === 400,
      });
    });
  });

  // ==================== HISTORY TESTS ====================
  group("HISTORY Operations", function () {
    if (createdIds.length === 0) {
      console.warn("No patients created, skipping history tests");
      return;
    }

    const testPatientId = createdIds[0];

    // Test 1: Read instance history
    group("Read Patient history", function () {
      const response = readHistory("Patient", testPatientId);

      if (
        check(response, {
          "History returns 200": (r) => r.status === 200,
          "History returns Bundle": (r) => {
            try {
              return JSON.parse(r.body).resourceType === "Bundle";
            } catch {
              return false;
            }
          },
        })
      ) {
        const body = JSON.parse(response.body);
        check(body, {
          "History has multiple versions": (b) =>
            b.entry && b.entry.length > 1,
          "History Bundle type is history": (b) => b.type === "history",
        });
      }
    });

    // Test 2: Vread specific version
    group("Vread specific version", function () {
      // First get history to find a version
      const historyResponse = readHistory("Patient", testPatientId);
      if (historyResponse.status === 200) {
        const history = JSON.parse(historyResponse.body);
        if (history.entry && history.entry.length > 0) {
          const firstVersion =
            history.entry[history.entry.length - 1].resource.meta.versionId;

          const response = vreadResource(
            "Patient",
            testPatientId,
            firstVersion
          );

          check(response, {
            "Vread returns 200": (r) => r.status === 200,
            "Vread returns correct version": (r) => {
              try {
                const body = JSON.parse(r.body);
                return body.meta.versionId === firstVersion;
              } catch {
                return false;
              }
            },
          });
        }
      }
    });
  });

  // ==================== DELETE TESTS ====================
  group("DELETE Operations", function () {
    if (createdIds.length === 0) {
      console.warn("No patients created, skipping delete tests");
      return;
    }

    // Test 1: Delete existing Patient
    group("Delete existing Patient", function () {
      const idToDelete = createdIds.pop();
      const start = Date.now();
      const response = deleteResource("Patient", idToDelete);
      deleteDuration.add(Date.now() - start);

      checkDeleted(response, "Patient");

      // Verify deletion - should return 404 or 410 (Gone)
      const readResponse = readResource("Patient", idToDelete);
      checkNotFound(readResponse, "Deleted Patient");
    });

    // Test 2: Delete non-existent Patient
    group("Delete non-existent Patient", function () {
      const start = Date.now();
      const response = deleteResource("Patient", "non-existent-delete-test");
      deleteDuration.add(Date.now() - start);

      // Server might return 204 (success - nothing to delete) or 404
      check(response, {
        "Delete non-existent returns valid status": (r) =>
          r.status === 204 || r.status === 200 || r.status === 404,
      });
    });

    // Test 3: Delete all remaining test Patients
    group("Cleanup - delete remaining Patients", function () {
      while (createdIds.length > 0) {
        const id = createdIds.pop();
        const start = Date.now();
        const response = deleteResource("Patient", id);
        deleteDuration.add(Date.now() - start);
        checkDeleted(response, `Cleanup Patient ${id}`);
      }
    });
  });

  crudCycles.add(1);
}

// Setup function - runs once before all iterations
export function setup() {
  console.log(`Testing against: ${config.baseUrl}`);

  // Verify server is reachable
  const response = searchResources("Patient", { _count: 1 });
  if (response.status !== 200) {
    fail(`Server not reachable at ${config.baseUrl}. Status: ${response.status}`);
  }

  return { startTime: Date.now() };
}

// Teardown function - runs once after all iterations
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nTest completed in ${duration.toFixed(2)}s`);
}
