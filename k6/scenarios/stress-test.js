// Stress test with gradual load increase

import http from "k6/http";
import { check, sleep } from "k6";
import { config, httpParams } from "../lib/config.js";
import { fixtures } from "../lib/fixtures.js";
import { randomElement, randomSleep } from "../lib/utils.js";

export const options = {
  stages: [
    { duration: "2m", target: 10 },   // Ramp up to 10 users
    { duration: "5m", target: 50 },   // Normal load: 50 users
    { duration: "2m", target: 100 },  // Peak load: 100 users
    { duration: "5m", target: 100 },  // Sustained peak
    { duration: "2m", target: 0 },    // Ramp down
  ],
  thresholds: {
    http_req_failed: ["rate<0.01"],      // <1% errors
    http_req_duration: ["p(95)<2000"],   // 95% < 2s
    http_req_duration: ["p(99)<5000"],   // 99% < 5s
  },
  tags: {
    test: "stress-test",
  },
};

const operations = [
  // Read operations (60%)
  { weight: 30, fn: searchPatients },
  { weight: 20, fn: readRandomPatient },
  { weight: 10, fn: readObservations },

  // Write operations (30%)
  { weight: 15, fn: createPatient },
  { weight: 10, fn: createObservation },
  { weight: 5, fn: updatePatient },

  // Complex operations (10%)
  { weight: 5, fn: bundleTransaction },
  { weight: 5, fn: systemHistory },
];

// Calculate cumulative weights
let cumulative = 0;
const weightedOps = operations.map((op) => {
  cumulative += op.weight;
  return { ...op, cumulative };
});

function selectOperation() {
  const rand = Math.random() * 100;
  return weightedOps.find((op) => rand <= op.cumulative).fn;
}

export default function () {
  const operation = selectOperation();
  operation();
  sleep(randomSleep(0.5, 2));
}

function searchPatients() {
  const queries = ["?name=John", "?gender=male", "?birthdate=ge1990-01-01", "?_count=20"];
  const query = randomElement(queries);

  http.get(`${config.baseUrl}/Patient${query}`, httpParams);
}

function readRandomPatient() {
  const id = Math.floor(Math.random() * 1000);
  http.get(`${config.baseUrl}/Patient/${id}`, {
    ...httpParams,
    tags: { name: `${config.baseUrl}/Patient/{id}` },
  });
}

function readObservations() {
  http.get(`${config.baseUrl}/Observation?_count=10`, httpParams);
}

function createPatient() {
  const patient = JSON.parse(JSON.stringify(fixtures.patient));
  http.post(`${config.baseUrl}/Patient`, JSON.stringify(patient), httpParams);
}

function createObservation() {
  const obs = JSON.parse(JSON.stringify(fixtures.observation));
  http.post(`${config.baseUrl}/Observation`, JSON.stringify(obs), httpParams);
}

function updatePatient() {
  const id = Math.floor(Math.random() * 1000);
  const response = http.get(`${config.baseUrl}/Patient/${id}`, {
    ...httpParams,
    tags: { name: `${config.baseUrl}/Patient/{id}` },
  });

  if (response.status === 200) {
    const patient = JSON.parse(response.body);
    patient.meta.lastUpdated = new Date().toISOString();
    http.put(`${config.baseUrl}/Patient/${id}`, JSON.stringify(patient), {
      ...httpParams,
      tags: { name: `${config.baseUrl}/Patient/{id}` },
    });
  }
}

function bundleTransaction() {
  const bundle = {
    resourceType: "Bundle",
    type: "transaction",
    entry: [
      {
        request: { method: "POST", url: "Patient" },
        resource: fixtures.patient,
      },
      {
        request: { method: "POST", url: "Observation" },
        resource: fixtures.observation,
      },
    ],
  };

  http.post(`${config.baseUrl}/`, JSON.stringify(bundle), httpParams);
}

function systemHistory() {
  http.get(`${config.baseUrl}/_history?_count=10`, httpParams);
}
