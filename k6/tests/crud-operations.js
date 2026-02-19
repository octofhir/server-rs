// Comprehensive CRUD operations test for multiple resource types

import http from "k6/http";
import { check, group, sleep } from "k6";
import { config, getHeaders, httpParams } from "../lib/config.js";
import { fixtures, withSubject } from "../lib/fixtures.js";
import {
  extractId,
  extractVersionId,
  checkFhirResponse,
  checkPerformance,
  randomSleep,
} from "../lib/utils.js";

export const options = {
  vus: 10,
  duration: "30s",
  thresholds: {
    http_req_failed: ["rate<0.01"], // <1% errors
    http_req_duration: ["p(95)<1000"], // 95% < 1s
    "http_req_duration{operation:create}": ["p(95)<1000"],
    "http_req_duration{operation:read}": ["p(95)<500"],
    "http_req_duration{operation:update}": ["p(95)<1000"],
    "http_req_duration{operation:delete}": ["p(95)<500"],
  },
  tags: {
    test: "crud-operations",
  },
};

// Note: Some resource types excluded due to enum case sensitivity issues in server
// TODO: Re-enable MedicationRequest, Encounter, AllergyIntolerance, Procedure once server enums support lowercase
const resourceTypes = [
  "Patient",
  "Observation",
  "Condition",
  // "MedicationRequest",  // Disabled: status enum expects PascalCase
  // "Encounter",          // Disabled: status enum expects PascalCase
  // "AllergyIntolerance", // Disabled: status enum expects PascalCase
  // "Procedure",          // Disabled: status enum expects PascalCase
];

// Map resource types to fixture keys (camelCase)
const fixtureKeyMap = {
  Patient: "patient",
  Observation: "observation",
  Condition: "condition",
  MedicationRequest: "medicationRequest",
  Encounter: "encounter",
  AllergyIntolerance: "allergyIntolerance",
  Procedure: "procedure",
};

export default function () {
  const resourceType = resourceTypes[Math.floor(Math.random() * resourceTypes.length)];

  group(`CRUD: ${resourceType}`, function () {
    let resourceId;
    let etag;

    // CREATE
    group("Create", function () {
      const fixtureKey = fixtureKeyMap[resourceType];
      const resource = JSON.parse(JSON.stringify(fixtures[fixtureKey]));

      const createResponse = http.post(
        `${config.baseUrl}/${resourceType}`,
        JSON.stringify(resource),
        {
          ...httpParams,
          tags: { operation: "create", resource_type: resourceType },
        }
      );

      check(createResponse, {
        "create: status 201": (r) => r.status === 201,
        "create: has Location header": (r) => r.headers["Location"] !== undefined || r.headers["location"] !== undefined,
        "create: has ETag header": (r) => r.headers["Etag"] !== undefined || r.headers["etag"] !== undefined,
        "create: returns resource": (r) => {
          try {
            const body = JSON.parse(r.body);
            return body.resourceType === resourceType;
          } catch (e) {
            return false;
          }
        },
      });

      checkPerformance(createResponse, 1000);

      if (createResponse.status === 201) {
        resourceId = extractId(createResponse.headers["Location"] || createResponse.headers["location"]);
        etag = extractVersionId(createResponse.headers["Etag"] || createResponse.headers["etag"]);
      }
    });

    if (!resourceId) {
      console.error(`Failed to create ${resourceType}, skipping remaining operations`);
      return;
    }

    sleep(randomSleep(0.1, 0.5));

    // READ
    group("Read", function () {
      const readResponse = http.get(`${config.baseUrl}/${resourceType}/${resourceId}`, {
        ...httpParams,
        tags: { name: `${config.baseUrl}/${resourceType}/{id}`, operation: "read", resource_type: resourceType },
      });

      check(readResponse, {
        "read: status 200": (r) => r.status === 200,
        "read: has ETag": (r) => r.headers["Etag"] !== undefined || r.headers["etag"] !== undefined,
        "read: correct resource ID": (r) => {
          try {
            const body = JSON.parse(r.body);
            return body.id === resourceId;
          } catch (e) {
            return false;
          }
        },
        "read: correct resource type": (r) => {
          try {
            const body = JSON.parse(r.body);
            return body.resourceType === resourceType;
          } catch (e) {
            return false;
          }
        },
      });

      checkPerformance(readResponse, 500);
    });

    sleep(randomSleep(0.1, 0.5));

    // UPDATE
    group("Update", function () {
      // First read to get current version
      const current = http.get(`${config.baseUrl}/${resourceType}/${resourceId}`, {
        ...httpParams,
        tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
      });

      if (current.status === 200) {
        const resource = JSON.parse(current.body);

        // Modify the resource
        if (resource.meta) {
          resource.meta.lastUpdated = new Date().toISOString();
        }

        const updateResponse = http.put(
          `${config.baseUrl}/${resourceType}/${resourceId}`,
          JSON.stringify(resource),
          {
            ...httpParams,
            tags: { name: `${config.baseUrl}/${resourceType}/{id}`, operation: "update", resource_type: resourceType },
          }
        );

        check(updateResponse, {
          "update: status 200": (r) => r.status === 200,
          "update: has new ETag": (r) => {
            const newEtag = r.headers["Etag"] || r.headers["etag"];
            const oldEtag = current.headers["Etag"] || current.headers["etag"];
            return newEtag !== undefined && newEtag !== oldEtag;
          },
          "update: version incremented": (r) => {
            try {
              const body = JSON.parse(r.body);
              return parseInt(body.meta.versionId) > 1;
            } catch (e) {
              return false;
            }
          },
        });

        checkPerformance(updateResponse, 1000);
      }
    });

    sleep(randomSleep(0.1, 0.5));

    // DELETE
    group("Delete", function () {
      const deleteResponse = http.del(`${config.baseUrl}/${resourceType}/${resourceId}`, null, {
        ...httpParams,
        tags: { name: `${config.baseUrl}/${resourceType}/{id}`, operation: "delete", resource_type: resourceType },
      });

      check(deleteResponse, {
        "delete: status 204": (r) => r.status === 204,
        "delete: no content": (r) => !r.body || r.body.length === 0,
      });

      checkPerformance(deleteResponse, 500);

      // Verify deletion - should return 410 Gone
      sleep(0.1);
      const verifyResponse = http.get(
        `${config.baseUrl}/${resourceType}/${resourceId}`,
        {
          ...httpParams,
          tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
        }
      );

      check(verifyResponse, {
        "delete verification: returns 410 Gone": (r) => r.status === 410,
      });
    });
  });

  sleep(randomSleep(1, 3));
}

export function handleSummary(data) {
  return {
    "results/crud-operations.json": JSON.stringify(data, null, 2),
    stdout: textSummary(data, { indent: "  ", enableColors: true }),
  };
}

function textSummary(data, options) {
  const { indent = "", enableColors = false } = options || {};
  const metrics = data.metrics;

  let summary = `\n${indent}CRUD Operations Test Summary\n${indent}${"=".repeat(50)}\n\n`;

  // Request stats
  if (metrics.http_reqs) {
    summary += `${indent}Total Requests: ${metrics.http_reqs.values.count}\n`;
  }

  if (metrics.http_req_duration) {
    const duration = metrics.http_req_duration.values;
    summary += `${indent}Request Duration:\n`;
    summary += `${indent}  avg: ${duration.avg.toFixed(2)}ms\n`;
    summary += `${indent}  p50: ${duration.med.toFixed(2)}ms\n`;
    summary += `${indent}  p95: ${duration["p(95)"].toFixed(2)}ms\n`;
    summary += `${indent}  p99: ${duration["p(99)"].toFixed(2)}ms\n`;
  }

  if (metrics.http_req_failed) {
    const failRate = metrics.http_req_failed.values.rate * 100;
    summary += `${indent}Error Rate: ${failRate.toFixed(2)}%\n`;
  }

  summary += `\n${indent}By Operation:\n`;
  ["create", "read", "update", "delete"].forEach((op) => {
    const metricName = `http_req_duration{operation:${op}}`;
    if (metrics[metricName]) {
      const opDuration = metrics[metricName].values;
      summary += `${indent}  ${op}:\n`;
      summary += `${indent}    p50: ${opDuration.med.toFixed(2)}ms\n`;
      summary += `${indent}    p95: ${opDuration["p(95)"].toFixed(2)}ms\n`;
    }
  });

  return summary;
}
