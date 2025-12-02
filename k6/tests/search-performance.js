// Search performance test covering simple, complex, and chained searches

import http from "k6/http";
import { check, group, sleep } from "k6";
import { config, httpParams } from "../lib/config.js";
import { checkBundle, checkPerformance, randomSleep, randomElement } from "../lib/utils.js";

export const options = {
  vus: 20,
  duration: "1m",
  thresholds: {
    http_req_failed: ["rate<0.01"],
    "http_req_duration{search_type:simple}": ["p(95)<1000"],
    "http_req_duration{search_type:complex}": ["p(95)<2000"],
    "http_req_duration{search_type:chained}": ["p(95)<3000"],
  },
  tags: {
    test: "search-performance",
  },
};

const simpleSearches = [
  "Patient?name=John",
  "Patient?gender=male",
  "Patient?birthdate=ge1990-01-01",
  "Observation?code=15074-8",
  "Observation?status=final",
  "Condition?clinical-status=active",
  "MedicationRequest?status=active",
  "Encounter?status=finished",
];

const complexSearches = [
  "Patient?name=John&birthdate=ge1990-01-01&gender=male",
  "Observation?code=15074-8&date=ge2023-01-01&value-quantity=gt5.0",
  "Condition?code=44054006&clinical-status=active&verification-status=confirmed",
  "MedicationRequest?status=active&intent=order&_count=50",
  "Patient?_has:Observation:patient:code=15074-8",
];

const chainedSearches = [
  "Observation?subject:Patient.name=John",
  "Observation?subject:Patient.birthdate=1990-01-01",
  "MedicationRequest?subject:Patient.gender=male",
  "Condition?subject:Patient.name=Smith",
];

export default function () {
  // Simple searches (60%)
  if (Math.random() < 0.6) {
    group("Simple Search", function () {
      const query = randomElement(simpleSearches);
      const response = http.get(`${config.baseUrl}/${query}`, {
        ...httpParams,
        tags: { search_type: "simple" },
      });

      check(response, {
        "simple search: status 200": (r) => r.status === 200,
      });

      checkBundle(response, "searchset");
      checkPerformance(response, 1000);
    });
  }
  // Complex searches (30%)
  else if (Math.random() < 0.85) {
    group("Complex Search", function () {
      const query = randomElement(complexSearches);
      const response = http.get(`${config.baseUrl}/${query}`, {
        ...httpParams,
        tags: { search_type: "complex" },
      });

      check(response, {
        "complex search: status 200": (r) => r.status === 200,
      });

      checkBundle(response, "searchset");
      checkPerformance(response, 2000);
    });
  }
  // Chained searches (10%)
  else {
    group("Chained Search", function () {
      const query = randomElement(chainedSearches);
      const response = http.get(`${config.baseUrl}/${query}`, {
        ...httpParams,
        tags: { search_type: "chained" },
      });

      check(response, {
        "chained search: status 200": (r) => r.status === 200,
      });

      checkBundle(response, "searchset");
      checkPerformance(response, 3000);
    });
  }

  sleep(randomSleep(0.5, 2));
}
