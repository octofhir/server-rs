// k6 Test Configuration

// Server configuration - can be overridden via environment variables
export const config = {
  baseUrl: __ENV.BASE_URL || "http://localhost:8888",
  timeout: __ENV.TIMEOUT || "30s",
  // Auth token if authentication is enabled
  authToken: __ENV.AUTH_TOKEN || null,
};

// Common HTTP headers for FHIR requests
export const fhirHeaders = {
  "Content-Type": "application/fhir+json",
  Accept: "application/fhir+json",
  // Skip validation for performance testing
  "X-Skip-Validation": "true",
};

// Get headers with optional auth
export function getHeaders() {
  const headers = { ...fhirHeaders };
  if (config.authToken) {
    headers["Authorization"] = `Bearer ${config.authToken}`;
  }
  return headers;
}

// Common request parameters
export const httpParams = {
  headers: fhirHeaders,
  timeout: config.timeout,
};

// Thresholds for different test scenarios
export const thresholds = {
  // Functional tests - focus on correctness
  functional: {
    http_req_failed: ["rate<0.01"], // Less than 1% errors
    http_req_duration: ["p(95)<2000"], // 95% under 2s
  },

  // Performance tests - stricter timing
  performance: {
    http_req_failed: ["rate<0.01"],
    http_req_duration: ["p(50)<100", "p(95)<500", "p(99)<1000"],
    http_reqs: ["rate>100"], // At least 100 req/s
  },

  // Load tests - sustained load
  load: {
    http_req_failed: ["rate<0.05"], // Less than 5% errors under load
    http_req_duration: ["p(95)<1000"],
  },

  // Stress tests - high load
  stress: {
    http_req_failed: ["rate<0.10"], // Allow up to 10% errors
    http_req_duration: ["p(95)<3000"],
  },
};

// Test scenarios
export const scenarios = {
  // Smoke test - single user, few iterations
  smoke: {
    executor: "per-vu-iterations",
    vus: 1,
    iterations: 5,
    maxDuration: "1m",
  },

  // Functional test - verify correctness
  functional: {
    executor: "per-vu-iterations",
    vus: 1,
    iterations: 1,
    maxDuration: "5m",
  },

  // Performance baseline - moderate load
  performance: {
    executor: "constant-vus",
    vus: 10,
    duration: "1m",
  },

  // Load test - ramp up and sustain
  load: {
    executor: "ramping-vus",
    startVUs: 0,
    stages: [
      { duration: "30s", target: 20 }, // Ramp up
      { duration: "2m", target: 20 }, // Sustain
      { duration: "30s", target: 0 }, // Ramp down
    ],
    gracefulRampDown: "10s",
  },

  // Stress test - find breaking point
  stress: {
    executor: "ramping-vus",
    startVUs: 0,
    stages: [
      { duration: "1m", target: 50 },
      { duration: "2m", target: 50 },
      { duration: "1m", target: 100 },
      { duration: "2m", target: 100 },
      { duration: "1m", target: 0 },
    ],
    gracefulRampDown: "30s",
  },

  // Spike test - sudden traffic spike
  spike: {
    executor: "ramping-vus",
    startVUs: 0,
    stages: [
      { duration: "10s", target: 5 }, // Normal load
      { duration: "10s", target: 100 }, // Spike
      { duration: "30s", target: 100 }, // Sustain spike
      { duration: "10s", target: 5 }, // Scale down
      { duration: "30s", target: 5 }, // Recovery
    ],
  },

  // Soak test - long duration
  soak: {
    executor: "constant-vus",
    vus: 10,
    duration: "30m",
  },
};
