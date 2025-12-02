// Patient Performance and Load Tests
// Run with different scenarios:
//   k6 run --env SCENARIO=smoke k6/tests/patient-performance.js
//   k6 run --env SCENARIO=performance k6/tests/patient-performance.js
//   k6 run --env SCENARIO=load k6/tests/patient-performance.js
//   k6 run --env SCENARIO=stress k6/tests/patient-performance.js
//   k6 run --env SCENARIO=spike k6/tests/patient-performance.js

import { check, sleep, group } from "k6";
import { Counter, Rate, Trend } from "k6/metrics";
import { SharedArray } from "k6/data";
import { config, thresholds, scenarios } from "../lib/config.js";
import {
  createResource,
  readResource,
  updateResource,
  deleteResource,
  searchResources,
  extractResourceId,
  checkCreated,
  checkRead,
  checkUpdated,
  checkDeleted,
  checkSearchBundle,
} from "../lib/fhir.js";
import { generateRandomPatient, validPatients } from "../data/patients.js";

// Pre-generate patients for consistent testing
const preGeneratedPatients = new SharedArray("patients", function () {
  const patients = [];
  for (let i = 0; i < 100; i++) {
    patients.push(generateRandomPatient());
  }
  return patients;
});

// Custom metrics
const createLatency = new Trend("patient_create_latency", true);
const readLatency = new Trend("patient_read_latency", true);
const updateLatency = new Trend("patient_update_latency", true);
const deleteLatency = new Trend("patient_delete_latency", true);
const searchLatency = new Trend("patient_search_latency", true);

const createSuccess = new Rate("patient_create_success");
const readSuccess = new Rate("patient_read_success");
const updateSuccess = new Rate("patient_update_success");
const deleteSuccess = new Rate("patient_delete_success");
const searchSuccess = new Rate("patient_search_success");

const totalOperations = new Counter("total_operations");

// Dynamic scenario selection based on environment variable
const selectedScenario = __ENV.SCENARIO || "smoke";

export const options = {
  scenarios: {
    [selectedScenario]: {
      ...scenarios[selectedScenario],
      exec: "performanceTest",
    },
  },
  thresholds: {
    // General thresholds
    http_req_failed: ["rate<0.05"],
    http_req_duration: ["p(95)<2000"],

    // Operation-specific thresholds
    patient_create_latency: ["p(95)<1000"],
    patient_read_latency: ["p(95)<500"],
    patient_update_latency: ["p(95)<1000"],
    patient_delete_latency: ["p(95)<500"],
    patient_search_latency: ["p(95)<1000"],

    // Success rates
    patient_create_success: ["rate>0.95"],
    patient_read_success: ["rate>0.99"],
    patient_update_success: ["rate>0.95"],
    patient_delete_success: ["rate>0.95"],
    patient_search_success: ["rate>0.99"],
  },
};

// Main performance test function
export function performanceTest() {
  const vuId = __VU;
  const iteration = __ITER;

  // Select a patient from pre-generated list
  const patientIndex = (vuId * 100 + iteration) % preGeneratedPatients.length;
  const basePatient = preGeneratedPatients[patientIndex];

  // Add unique identifier for this VU/iteration
  const patient = {
    ...basePatient,
    identifier: [
      {
        system: "http://k6-perf-test.example.org",
        value: `vu${vuId}-iter${iteration}-${Date.now()}`,
      },
    ],
  };

  let patientId = null;

  // CREATE
  group("CREATE Performance", function () {
    const start = Date.now();
    const response = createResource("Patient", patient);
    const duration = Date.now() - start;

    createLatency.add(duration);
    totalOperations.add(1);

    const success = response.status === 201;
    createSuccess.add(success);

    if (success) {
      patientId = extractResourceId(response);
    }
  });

  // Skip remaining operations if create failed
  if (!patientId) {
    return;
  }

  // Small delay to ensure resource is available
  sleep(0.05);

  // READ
  group("READ Performance", function () {
    const start = Date.now();
    const response = readResource("Patient", patientId);
    const duration = Date.now() - start;

    readLatency.add(duration);
    totalOperations.add(1);

    const success = response.status === 200;
    readSuccess.add(success);
  });

  // UPDATE
  group("UPDATE Performance", function () {
    const updatedPatient = {
      ...patient,
      id: patientId,
      active: !patient.active,
      name: [
        {
          ...patient.name[0],
          family: `${patient.name[0].family}-Updated`,
        },
      ],
    };

    const start = Date.now();
    const response = updateResource("Patient", patientId, updatedPatient);
    const duration = Date.now() - start;

    updateLatency.add(duration);
    totalOperations.add(1);

    const success = response.status === 200;
    updateSuccess.add(success);
  });

  // SEARCH
  group("SEARCH Performance", function () {
    // Random search parameter
    const searchTypes = [
      { family: patient.name[0].family },
      { gender: patient.gender },
      { _count: 10 },
      { active: patient.active ? "true" : "false" },
    ];
    const searchParams = searchTypes[iteration % searchTypes.length];

    const start = Date.now();
    const response = searchResources("Patient", searchParams);
    const duration = Date.now() - start;

    searchLatency.add(duration);
    totalOperations.add(1);

    const success = response.status === 200;
    searchSuccess.add(success);
  });

  // DELETE (cleanup)
  group("DELETE Performance", function () {
    const start = Date.now();
    const response = deleteResource("Patient", patientId);
    const duration = Date.now() - start;

    deleteLatency.add(duration);
    totalOperations.add(1);

    const success = response.status === 200 || response.status === 204;
    deleteSuccess.add(success);
  });

  // Small think time between iterations
  sleep(0.1 + Math.random() * 0.2);
}

// Mixed workload test - simulates realistic usage patterns
export function mixedWorkload() {
  const vuId = __VU;
  const iteration = __ITER;

  // Workload distribution:
  // 50% reads, 30% searches, 15% creates, 5% updates
  const rand = Math.random() * 100;

  if (rand < 50) {
    // READ operation - read existing or recently created
    const response = searchResources("Patient", { _count: 1 });
    if (response.status === 200) {
      try {
        const bundle = JSON.parse(response.body);
        if (bundle.entry && bundle.entry.length > 0) {
          const patientId = bundle.entry[0].resource.id;
          const start = Date.now();
          const readResponse = readResource("Patient", patientId);
          readLatency.add(Date.now() - start);
          readSuccess.add(readResponse.status === 200);
          totalOperations.add(1);
        }
      } catch (e) {
        // Ignore parse errors
      }
    }
  } else if (rand < 80) {
    // SEARCH operation
    const searchParams = [
      { gender: "male" },
      { gender: "female" },
      { active: "true" },
      { _count: 20 },
    ][Math.floor(Math.random() * 4)];

    const start = Date.now();
    const response = searchResources("Patient", searchParams);
    searchLatency.add(Date.now() - start);
    searchSuccess.add(response.status === 200);
    totalOperations.add(1);
  } else if (rand < 95) {
    // CREATE operation
    const patient = {
      ...preGeneratedPatients[iteration % preGeneratedPatients.length],
      identifier: [
        {
          system: "http://k6-mixed-test.example.org",
          value: `mixed-vu${vuId}-${Date.now()}`,
        },
      ],
    };

    const start = Date.now();
    const response = createResource("Patient", patient);
    createLatency.add(Date.now() - start);
    createSuccess.add(response.status === 201);
    totalOperations.add(1);
  } else {
    // UPDATE operation - find and update a patient
    const searchResponse = searchResources("Patient", { _count: 1 });
    if (searchResponse.status === 200) {
      try {
        const bundle = JSON.parse(searchResponse.body);
        if (bundle.entry && bundle.entry.length > 0) {
          const existingPatient = bundle.entry[0].resource;
          const updatedPatient = {
            ...existingPatient,
            active: !existingPatient.active,
          };

          const start = Date.now();
          const updateResponse = updateResource(
            "Patient",
            existingPatient.id,
            updatedPatient
          );
          updateLatency.add(Date.now() - start);
          updateSuccess.add(updateResponse.status === 200);
          totalOperations.add(1);
        }
      } catch (e) {
        // Ignore parse errors
      }
    }
  }

  sleep(0.05 + Math.random() * 0.1);
}

// Read-heavy workload
export function readHeavyWorkload() {
  const iteration = __ITER;

  // First, ensure we have patients to read
  if (iteration === 0) {
    // Create some patients at the start
    for (let i = 0; i < 5; i++) {
      const patient = {
        ...preGeneratedPatients[i],
        identifier: [
          {
            system: "http://k6-read-test.example.org",
            value: `read-test-${i}`,
          },
        ],
      };
      createResource("Patient", patient);
    }
  }

  // Perform multiple reads
  for (let i = 0; i < 10; i++) {
    const searchResponse = searchResources("Patient", {
      _count: 1,
      _offset: i % 5,
    });

    if (searchResponse.status === 200) {
      try {
        const bundle = JSON.parse(searchResponse.body);
        if (bundle.entry && bundle.entry.length > 0) {
          const patientId = bundle.entry[0].resource.id;
          const start = Date.now();
          const response = readResource("Patient", patientId);
          readLatency.add(Date.now() - start);
          readSuccess.add(response.status === 200);
          totalOperations.add(1);
        }
      } catch (e) {
        // Ignore
      }
    }
  }

  sleep(0.1);
}

// Search stress test
export function searchStress() {
  const searchParams = [
    {},
    { gender: "male" },
    { gender: "female" },
    { active: "true" },
    { active: "false" },
    { _count: 5 },
    { _count: 20 },
    { _count: 50 },
  ];

  for (const params of searchParams) {
    const start = Date.now();
    const response = searchResources("Patient", params);
    searchLatency.add(Date.now() - start);
    searchSuccess.add(response.status === 200);
    totalOperations.add(1);
  }

  sleep(0.2);
}

// Setup function
export function setup() {
  console.log(`\nPerformance Test Configuration:`);
  console.log(`  Server: ${config.baseUrl}`);
  console.log(`  Scenario: ${selectedScenario}`);
  console.log(`  Pre-generated patients: ${preGeneratedPatients.length}`);

  // Verify server is reachable
  const response = searchResources("Patient", { _count: 1 });
  if (response.status !== 200) {
    console.error(`Server not reachable at ${config.baseUrl}`);
  }

  return {
    startTime: Date.now(),
    scenario: selectedScenario,
  };
}

// Teardown function
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\n========================================`);
  console.log(`Performance Test Summary`);
  console.log(`========================================`);
  console.log(`Scenario: ${data.scenario}`);
  console.log(`Duration: ${duration.toFixed(2)}s`);
  console.log(`========================================`);
}

// Custom summary handler
export function handleSummary(data) {
  const summary = {
    scenario: selectedScenario,
    timestamp: new Date().toISOString(),
    metrics: {
      total_requests: data.metrics.http_reqs?.values?.count || 0,
      failed_requests: data.metrics.http_req_failed?.values?.rate || 0,
      avg_duration: data.metrics.http_req_duration?.values?.avg || 0,
      p95_duration: data.metrics.http_req_duration?.values["p(95)"] || 0,
      p99_duration: data.metrics.http_req_duration?.values["p(99)"] || 0,
      operations: {
        create: {
          p95: data.metrics.patient_create_latency?.values["p(95)"] || 0,
          success_rate:
            data.metrics.patient_create_success?.values?.rate || 0,
        },
        read: {
          p95: data.metrics.patient_read_latency?.values["p(95)"] || 0,
          success_rate: data.metrics.patient_read_success?.values?.rate || 0,
        },
        update: {
          p95: data.metrics.patient_update_latency?.values["p(95)"] || 0,
          success_rate:
            data.metrics.patient_update_success?.values?.rate || 0,
        },
        delete: {
          p95: data.metrics.patient_delete_latency?.values["p(95)"] || 0,
          success_rate:
            data.metrics.patient_delete_success?.values?.rate || 0,
        },
        search: {
          p95: data.metrics.patient_search_latency?.values["p(95)"] || 0,
          success_rate:
            data.metrics.patient_search_success?.values?.rate || 0,
        },
      },
    },
  };

  return {
    stdout: textSummary(data, { indent: " ", enableColors: true }),
    "k6/results/summary.json": JSON.stringify(summary, null, 2),
  };
}

// Text summary generator
function textSummary(data, options) {
  let output = "\n";
  output += "╔══════════════════════════════════════════════════════════════╗\n";
  output += "║                    K6 Performance Summary                     ║\n";
  output += "╠══════════════════════════════════════════════════════════════╣\n";
  output += `║  Scenario: ${selectedScenario.padEnd(50)}║\n`;
  output += `║  Total Requests: ${String(data.metrics.http_reqs?.values?.count || 0).padEnd(44)}║\n`;
  output += `║  Failed Rate: ${String((data.metrics.http_req_failed?.values?.rate * 100 || 0).toFixed(2) + "%").padEnd(47)}║\n`;
  output += "╠══════════════════════════════════════════════════════════════╣\n";
  output += "║  Latency (p95):                                              ║\n";
  output += `║    Create: ${String((data.metrics.patient_create_latency?.values["p(95)"] || 0).toFixed(2) + "ms").padEnd(50)}║\n`;
  output += `║    Read:   ${String((data.metrics.patient_read_latency?.values["p(95)"] || 0).toFixed(2) + "ms").padEnd(50)}║\n`;
  output += `║    Update: ${String((data.metrics.patient_update_latency?.values["p(95)"] || 0).toFixed(2) + "ms").padEnd(50)}║\n`;
  output += `║    Delete: ${String((data.metrics.patient_delete_latency?.values["p(95)"] || 0).toFixed(2) + "ms").padEnd(50)}║\n`;
  output += `║    Search: ${String((data.metrics.patient_search_latency?.values["p(95)"] || 0).toFixed(2) + "ms").padEnd(50)}║\n`;
  output += "╚══════════════════════════════════════════════════════════════╝\n";

  return output;
}
