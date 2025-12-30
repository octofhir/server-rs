// OAuth authentication utilities for k6 tests
// Supports password grant for testing with real authentication

import http from "k6/http";
import { check } from "k6";
import { config } from "./config.js";

/**
 * OAuth token endpoint URL
 */
export const tokenEndpoint = `${config.baseUrl}/auth/token`;

/**
 * Default OAuth client ID (the built-in UI client that supports password grant)
 */
export const DEFAULT_CLIENT_ID = "octofhir-ui";

/**
 * Get an access token using the Resource Owner Password Credentials (ROPC) grant.
 *
 * @param {object} options - Authentication options
 * @param {string} options.username - Username for authentication
 * @param {string} options.password - Password for authentication
 * @param {string} [options.clientId] - OAuth client ID (defaults to octofhir-ui)
 * @param {string} [options.scope] - Requested scopes (defaults to openid system/*.cruds)
 * @returns {object} - Token response with access_token, or null on failure
 */
export function getAccessToken(options) {
  const { username, password, clientId = DEFAULT_CLIENT_ID, scope = "openid system/*.cruds" } = options;

  const payload = {
    grant_type: "password",
    username: username,
    password: password,
    client_id: clientId,
    scope: scope,
  };

  const response = http.post(tokenEndpoint, payload, {
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
    },
    timeout: "10s",
  });

  const success = check(response, {
    "Token request succeeded (200)": (r) => r.status === 200,
    "Token response has access_token": (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.access_token !== undefined;
      } catch {
        return false;
      }
    },
  });

  if (!success) {
    console.error(`Failed to get access token: ${response.status} ${response.body}`);
    return null;
  }

  try {
    return JSON.parse(response.body);
  } catch (e) {
    console.error(`Failed to parse token response: ${e}`);
    return null;
  }
}

/**
 * Create authorization header from access token
 *
 * @param {string} accessToken - The access token
 * @returns {object} - Headers object with Authorization header
 */
export function authHeaders(accessToken) {
  return {
    Authorization: `Bearer ${accessToken}`,
  };
}

/**
 * Get configured auth credentials from environment variables
 *
 * @returns {object|null} - Credentials object or null if not configured
 */
export function getEnvCredentials() {
  const username = __ENV.AUTH_USERNAME || __ENV.OCTOFHIR_USERNAME;
  const password = __ENV.AUTH_PASSWORD || __ENV.OCTOFHIR_PASSWORD;

  if (!username || !password) {
    return null;
  }

  return { username, password };
}

/**
 * Setup function to get and cache an access token.
 * Call this in your test's setup() function.
 *
 * @param {object} [credentials] - Optional credentials, or uses environment variables
 * @returns {object} - Setup data including accessToken
 */
export function authSetup(credentials = null) {
  // Try credentials parameter first, then environment variables, then defaults
  const creds = credentials || getEnvCredentials() || {
    username: "admin",
    password: "admin",
  };

  console.log(`Authenticating as user: ${creds.username}`);

  const tokenResponse = getAccessToken(creds);

  if (!tokenResponse) {
    console.warn("Authentication failed - tests will run without auth");
    return { accessToken: null, authenticated: false };
  }

  console.log(`Authentication successful, token expires in ${tokenResponse.expires_in}s`);

  return {
    accessToken: tokenResponse.access_token,
    refreshToken: tokenResponse.refresh_token,
    expiresIn: tokenResponse.expires_in,
    scope: tokenResponse.scope,
    authenticated: true,
  };
}

/**
 * Refresh an access token using a refresh token.
 *
 * @param {string} refreshToken - The refresh token
 * @param {string} [clientId] - OAuth client ID (defaults to octofhir-ui)
 * @returns {object|null} - New token response or null on failure
 */
export function refreshAccessToken(refreshToken, clientId = DEFAULT_CLIENT_ID) {
  const payload = {
    grant_type: "refresh_token",
    refresh_token: refreshToken,
    client_id: clientId,
  };

  const response = http.post(tokenEndpoint, payload, {
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
    },
    timeout: "10s",
  });

  if (response.status !== 200) {
    console.error(`Failed to refresh token: ${response.status} ${response.body}`);
    return null;
  }

  try {
    return JSON.parse(response.body);
  } catch (e) {
    console.error(`Failed to parse refresh token response: ${e}`);
    return null;
  }
}
