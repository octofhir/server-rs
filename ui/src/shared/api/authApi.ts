import type { AuthError, LogoutResponse, TokenResponse, UserInfo } from "./types";

/**
 * Default OAuth client ID for the admin UI.
 * This should match an existing client configured in the server.
 */
const DEFAULT_CLIENT_ID = "octofhir-ui";

class AuthApiClient {
  private baseUrl: string;
  private clientId: string;

  constructor(baseUrl = "", clientId = DEFAULT_CLIENT_ID) {
    this.baseUrl = baseUrl;
    this.clientId = clientId;
  }

  /**
   * Login using OAuth 2.0 Resource Owner Password Credentials Grant.
   *
   * On success, the server sets an HttpOnly cookie with the access token.
   * The cookie is automatically sent with subsequent requests.
   */
  async login(username: string, password: string): Promise<TokenResponse> {
    const url = `${this.baseUrl}/auth/token`;

    // OAuth 2.0 token endpoint expects application/x-www-form-urlencoded
    const body = new URLSearchParams({
      grant_type: "password",
      client_id: this.clientId,
      username,
      password,
      scope: "openid", // Request openid scope for userinfo endpoint
    });

    const response = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
      },
      body: body.toString(),
      credentials: "include", // Important: include cookies
    });

    const data = await response.json();

    if (!response.ok) {
      const authError = data as AuthError;
      throw new AuthApiError(
        authError.error_description || authError.error || "Login failed",
        authError.error || "unknown_error",
        response.status,
      );
    }

    return data as TokenResponse;
  }

  /**
   * Logout the current user.
   *
   * This revokes the access token and clears the auth cookie.
   */
  async logout(): Promise<LogoutResponse> {
    const url = `${this.baseUrl}/auth/logout`;

    const response = await fetch(url, {
      method: "POST",
      credentials: "include", // Important: include cookies
    });

    const data = await response.json();

    if (!response.ok) {
      throw new AuthApiError(
        data.error_description || data.error || "Logout failed",
        data.error || "unknown_error",
        response.status,
      );
    }

    return data as LogoutResponse;
  }

  /**
   * Get information about the currently authenticated user.
   *
   * Returns null if not authenticated or token is invalid.
   */
  async getCurrentUser(): Promise<UserInfo | null> {
    const url = `${this.baseUrl}/auth/userinfo`;

    try {
      const response = await fetch(url, {
        method: "GET",
        credentials: "include", // Important: include cookies
      });

      // 401 = not authenticated, 403 = token invalid/missing required scope
      if (response.status === 401 || response.status === 403) {
        return null;
      }

      if (!response.ok) {
        const data = await response.json();
        throw new AuthApiError(
          data.error_description || data.error || "Failed to get user info",
          data.error || "unknown_error",
          response.status,
        );
      }

      return (await response.json()) as UserInfo;
    } catch (error) {
      if (error instanceof AuthApiError) {
        throw error;
      }
      // Network error or other issue - treat as not authenticated
      return null;
    }
  }

  /**
   * Check if the current session is authenticated.
   *
   * This is a lightweight check that doesn't fetch full user info.
   */
  async checkAuth(): Promise<boolean> {
    const user = await this.getCurrentUser();
    return user !== null;
  }

  setBaseUrl(baseUrl: string): void {
    this.baseUrl = baseUrl;
  }

  setClientId(clientId: string): void {
    this.clientId = clientId;
  }
}

/**
 * Custom error class for authentication errors.
 */
export class AuthApiError extends Error {
  readonly code: string;
  readonly statusCode: number;

  constructor(message: string, code: string, statusCode: number) {
    super(message);
    this.name = "AuthApiError";
    this.code = code;
    this.statusCode = statusCode;
  }

  /**
   * Check if this is an invalid credentials error.
   */
  isInvalidCredentials(): boolean {
    return this.code === "invalid_grant" || this.code === "invalid_client";
  }

  /**
   * Check if this is a rate limiting error.
   */
  isRateLimited(): boolean {
    return this.statusCode === 429;
  }
}

// Default instance
export const authApi = new AuthApiClient();
export { AuthApiClient };
