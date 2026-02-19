// FHIR utility functions for k6 tests

import http from "k6/http";
import { check, fail } from "k6";
import { config, getHeaders, httpParams } from "./config.js";

// Extract resource ID from Location header or response body
export function extractResourceId(response) {
  // Try Location header first (format: /Patient/123/_history/1)
  const location = response.headers["Location"] || response.headers["location"];
  if (location) {
    const match = location.match(/\/([^/]+)\/([^/_]+)/);
    if (match) {
      return match[2];
    }
  }

  // Fall back to response body
  try {
    const body = JSON.parse(response.body);
    return body.id;
  } catch (e) {
    return null;
  }
}

// Extract version ID from ETag header or response body
export function extractVersionId(response) {
  const etag = response.headers["ETag"] || response.headers["etag"];
  if (etag) {
    // ETag format: W/"1" or "1"
    const match = etag.match(/"([^"]+)"/);
    if (match) {
      return match[1];
    }
  }

  try {
    const body = JSON.parse(response.body);
    return body.meta?.versionId;
  } catch (e) {
    return null;
  }
}

// FHIR CRUD Operations

// Create a resource
export function createResource(resourceType, resource) {
  const url = `${config.baseUrl}/${resourceType}`;
  const response = http.post(url, JSON.stringify(resource), {
    headers: getHeaders(),
    ...httpParams,
  });
  return response;
}

// Read a resource by ID
export function readResource(resourceType, id) {
  const url = `${config.baseUrl}/${resourceType}/${id}`;
  const response = http.get(url, {
    headers: getHeaders(),
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
  });
  return response;
}

// Update a resource (PUT)
export function updateResource(resourceType, id, resource) {
  const url = `${config.baseUrl}/${resourceType}/${id}`;
  const resourceWithId = { ...resource, id };
  const response = http.put(url, JSON.stringify(resourceWithId), {
    headers: getHeaders(),
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
  });
  return response;
}

// Delete a resource
export function deleteResource(resourceType, id) {
  const url = `${config.baseUrl}/${resourceType}/${id}`;
  const response = http.del(url, null, {
    headers: getHeaders(),
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
  });
  return response;
}

// Build query string from params object (k6 doesn't have URLSearchParams)
function buildQueryString(params) {
  const parts = [];
  for (const key in params) {
    if (Object.prototype.hasOwnProperty.call(params, key)) {
      const value = params[key];
      parts.push(encodeURIComponent(key) + "=" + encodeURIComponent(value));
    }
  }
  return parts.join("&");
}

// Search resources
export function searchResources(resourceType, params = {}) {
  const searchParams = buildQueryString(params);
  const url = `${config.baseUrl}/${resourceType}${searchParams ? "?" + searchParams : ""}`;
  const response = http.get(url, {
    headers: getHeaders(),
    ...httpParams,
  });
  return response;
}

// Read resource history
export function readHistory(resourceType, id) {
  const url = `${config.baseUrl}/${resourceType}/${id}/_history`;
  const response = http.get(url, {
    headers: getHeaders(),
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}/_history` },
  });
  return response;
}

// Read specific version (vread)
export function vreadResource(resourceType, id, versionId) {
  const url = `${config.baseUrl}/${resourceType}/${id}/_history/${versionId}`;
  const response = http.get(url, {
    headers: getHeaders(),
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}/_history/{vid}` },
  });
  return response;
}

// Patch resource (JSON Patch)
export function patchResource(resourceType, id, patchOperations) {
  const url = `${config.baseUrl}/${resourceType}/${id}`;
  const response = http.patch(url, JSON.stringify(patchOperations), {
    headers: {
      ...getHeaders(),
      "Content-Type": "application/json-patch+json",
    },
    ...httpParams,
    tags: { name: `${config.baseUrl}/${resourceType}/{id}` },
  });
  return response;
}

// Conditional create (POST with If-None-Exist)
export function conditionalCreate(resourceType, resource, searchParams) {
  const url = `${config.baseUrl}/${resourceType}`;
  const response = http.post(url, JSON.stringify(resource), {
    headers: {
      ...getHeaders(),
      "If-None-Exist": searchParams,
    },
    ...httpParams,
  });
  return response;
}

// Batch request
export function batchRequest(bundle) {
  const url = config.baseUrl;
  const response = http.post(url, JSON.stringify(bundle), {
    headers: getHeaders(),
    ...httpParams,
  });
  return response;
}

// Check helpers

// Check for successful creation
export function checkCreated(response, name = "Patient") {
  return check(response, {
    [`${name} created (201)`]: (r) => r.status === 201,
    [`${name} has Location header`]: (r) =>
      r.headers["Location"] || r.headers["location"],
    [`${name} has id in body`]: (r) => {
      try {
        return JSON.parse(r.body).id !== undefined;
      } catch {
        return false;
      }
    },
  });
}

// Check for successful read
export function checkRead(response, name = "Patient") {
  // Extract resource type from name (e.g., "Patient 123" -> "Patient")
  const resourceType = name.split(" ")[0];
  return check(response, {
    [`${name} read (200)`]: (r) => r.status === 200,
    [`${name} has correct resourceType`]: (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.resourceType === resourceType;
      } catch {
        return false;
      }
    },
  });
}

// Check for successful update
export function checkUpdated(response, name = "Patient") {
  return check(response, {
    [`${name} updated (200)`]: (r) => r.status === 200,
    [`${name} has updated versionId`]: (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.meta?.versionId !== undefined;
      } catch {
        return false;
      }
    },
  });
}

// Check for successful delete
export function checkDeleted(response, name = "Patient") {
  return check(response, {
    [`${name} deleted (204 or 200)`]: (r) =>
      r.status === 204 || r.status === 200,
  });
}

// Check for not found
export function checkNotFound(response, name = "Patient") {
  return check(response, {
    [`${name} not found (404 or 410)`]: (r) =>
      r.status === 404 || r.status === 410,
  });
}

// Check for validation error
export function checkValidationError(response, name = "Resource") {
  return check(response, {
    [`${name} rejected (400 or 422)`]: (r) =>
      r.status === 400 || r.status === 422,
    [`${name} returns OperationOutcome`]: (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.resourceType === "OperationOutcome";
      } catch {
        return false;
      }
    },
  });
}

// Check for search bundle
export function checkSearchBundle(response, name = "Search") {
  return check(response, {
    [`${name} returns 200`]: (r) => r.status === 200,
    [`${name} returns Bundle`]: (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.resourceType === "Bundle";
      } catch {
        return false;
      }
    },
    [`${name} Bundle has total`]: (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.total !== undefined;
      } catch {
        return false;
      }
    },
  });
}

// Parse Bundle and extract resources
export function extractBundleResources(response) {
  try {
    const body = JSON.parse(response.body);
    if (body.resourceType === "Bundle" && body.entry) {
      return body.entry.map((e) => e.resource);
    }
    return [];
  } catch {
    return [];
  }
}

// Generate a unique identifier for test isolation
export function generateTestId() {
  return `k6-${Date.now()}-${Math.random().toString(36).substring(7)}`;
}
