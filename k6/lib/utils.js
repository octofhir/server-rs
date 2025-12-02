// Utility functions for k6 FHIR tests

import { check } from "k6";

// Extract resource ID from Location header or full URL
export function extractId(location) {
  if (!location) return null;

  // Handle full URL: http://localhost:8888/Patient/123/_history/1
  // or relative URL: /Patient/123/_history/1
  const match = location.match(/\/([^/]+)\/([^/_]+)/);
  if (match) {
    return match[2];
  }

  // Fallback: just the ID itself
  return location;
}

// Extract version ID from ETag header
export function extractVersionId(etag) {
  if (!etag) return null;

  // ETag format: W/"1" or "1"
  const match = etag.match(/"([^"]+)"/);
  return match ? match[1] : null;
}

// Generate a random patient ID for testing
export function randomPatientId() {
  return `patient-${Math.floor(Math.random() * 10000)}`;
}

// Generate a random resource ID
export function randomId() {
  return Math.floor(Math.random() * 10000).toString();
}

// Parse JSON response safely
export function safeParseJSON(body) {
  try {
    return JSON.parse(body);
  } catch (e) {
    console.error("Failed to parse JSON:", e.message);
    return null;
  }
}

// Check if response is successful (2xx)
export function isSuccess(status) {
  return status >= 200 && status < 300;
}

// Standard checks for FHIR responses
export function checkFhirResponse(response, expectedStatus, operationName) {
  return check(response, {
    [`${operationName}: status ${expectedStatus}`]: (r) =>
      r.status === expectedStatus,
    [`${operationName}: has body`]: (r) => r.body && r.body.length > 0,
    [`${operationName}: is JSON`]: (r) => {
      try {
        JSON.parse(r.body);
        return true;
      } catch (e) {
        return false;
      }
    },
  });
}

// Check if response contains a valid FHIR Bundle
export function checkBundle(response, expectedType = null) {
  const checks = {
    "is Bundle": (r) => {
      const body = safeParseJSON(r.body);
      return body && body.resourceType === "Bundle";
    },
    "has entries": (r) => {
      const body = safeParseJSON(r.body);
      return body && Array.isArray(body.entry);
    },
  };

  if (expectedType) {
    checks[`bundle type is ${expectedType}`] = (r) => {
      const body = safeParseJSON(r.body);
      return body && body.type === expectedType;
    };
  }

  return check(response, checks);
}

// Check performance thresholds
export function checkPerformance(response, p95Threshold, p99Threshold = null) {
  const checks = {
    [`p95 < ${p95Threshold}ms`]: (r) => r.timings.duration < p95Threshold,
  };

  if (p99Threshold) {
    checks[`p99 < ${p99Threshold}ms`] = (r) =>
      r.timings.duration < p99Threshold;
  }

  return check(response, checks);
}

// Sleep with randomness to simulate real user behavior
export function randomSleep(min = 1, max = 3) {
  const duration = Math.random() * (max - min) + min;
  return duration;
}

// Create search query string from params object
export function buildSearchQuery(params) {
  const query = Object.entries(params)
    .map(([key, value]) => `${key}=${encodeURIComponent(value)}`)
    .join("&");
  return query ? `?${query}` : "";
}

// Format error message for logging
export function formatError(response) {
  let message = `Status: ${response.status}`;

  try {
    const body = JSON.parse(response.body);
    if (body.resourceType === "OperationOutcome") {
      const issues = body.issue
        .map((issue) => `${issue.severity}: ${issue.diagnostics}`)
        .join(", ");
      message += ` - ${issues}`;
    } else {
      message += ` - ${response.body.substring(0, 200)}`;
    }
  } catch (e) {
    message += ` - ${response.body.substring(0, 200)}`;
  }

  return message;
}

// Calculate success rate from metrics
export function calculateSuccessRate(successCount, totalCount) {
  if (totalCount === 0) return 0;
  return ((successCount / totalCount) * 100).toFixed(2);
}

// Wait for condition with timeout
export async function waitFor(conditionFn, timeout = 5000, interval = 100) {
  const startTime = Date.now();
  while (Date.now() - startTime < timeout) {
    if (conditionFn()) {
      return true;
    }
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
  return false;
}

// Batch array into chunks
export function chunk(array, size) {
  const chunks = [];
  for (let i = 0; i < array.length; i += size) {
    chunks.push(array.slice(i, i + size));
  }
  return chunks;
}

// Random element from array
export function randomElement(array) {
  return array[Math.floor(Math.random() * array.length)];
}

// Generate timestamp for unique resources
export function timestamp() {
  return new Date().toISOString();
}

// Format bytes to human readable
export function formatBytes(bytes) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return Math.round(bytes / Math.pow(k, i) * 100) / 100 + " " + sizes[i];
}

// Format duration in ms to human readable
export function formatDuration(ms) {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(2)}s`;
  return `${(ms / 60000).toFixed(2)}m`;
}
