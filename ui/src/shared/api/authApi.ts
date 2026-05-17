import type {
	AuthError,
	LogoutResponse,
	TokenResponse,
	UserInfo,
} from "./types";
import { isRecord } from "./guards";

/**
 * Default OAuth client ID for the admin UI.
 * This should match an existing client configured in the server.
 */
const DEFAULT_CLIENT_ID = "octofhir-ui";

async function readJson(response: Response): Promise<unknown> {
	const text = await response.text();
	return text ? JSON.parse(text) : undefined;
}

function readAuthError(value: unknown, fallback: string): AuthError {
	if (!isRecord(value)) {
		return { error: "unknown_error", error_description: fallback };
	}

	const error = typeof value.error === "string" ? value.error : "unknown_error";
	const errorDescription =
		typeof value.error_description === "string" ? value.error_description : fallback;

	return { error, error_description: errorDescription };
}

function isTokenResponse(value: unknown): value is TokenResponse {
	return (
		isRecord(value) &&
		typeof value.access_token === "string" &&
		value.token_type === "Bearer" &&
		typeof value.expires_in === "number" &&
		(value.refresh_token === undefined || typeof value.refresh_token === "string") &&
		(value.scope === undefined || typeof value.scope === "string")
	);
}

function isUserInfo(value: unknown): value is UserInfo {
	return (
		isRecord(value) &&
		typeof value.sub === "string" &&
		(value.name === undefined || typeof value.name === "string") &&
		(value.preferred_username === undefined || typeof value.preferred_username === "string") &&
		(value.email === undefined || typeof value.email === "string") &&
		(value.fhirUser === undefined || typeof value.fhirUser === "string") &&
		(value.roles === undefined ||
			(Array.isArray(value.roles) && value.roles.every((role) => typeof role === "string")))
	);
}

function isLogoutResponse(value: unknown): value is LogoutResponse {
	return (
		isRecord(value) &&
		typeof value.success === "boolean" &&
		typeof value.message === "string"
	);
}

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
			scope: "openid offline_access user/*.cruds system/*.cruds",
		});

		const response = await fetch(url, {
			method: "POST",
			headers: {
				"Content-Type": "application/x-www-form-urlencoded",
			},
			body: body.toString(),
			credentials: "include", // Important: include cookies
		});

		const data = await readJson(response);

		if (!response.ok) {
			const authError = readAuthError(data, "Login failed");
			throw new AuthApiError(
				authError.error_description || authError.error || "Login failed",
				authError.error || "unknown_error",
				response.status,
			);
		}

		if (!isTokenResponse(data)) {
			throw new AuthApiError("Invalid login response", "invalid_response", response.status);
		}

		return data;
	}

	/**
	 * Refresh the access token using a refresh token.
	 *
	 * This endpoint may not be available on all servers.
	 * If the server doesn't support refresh tokens, this will fail.
	 */
	async refresh(refreshToken: string): Promise<TokenResponse> {
		const url = `${this.baseUrl}/auth/token`;

		const body = new URLSearchParams({
			grant_type: "refresh_token",
			client_id: this.clientId,
			refresh_token: refreshToken,
		});

		const response = await fetch(url, {
			method: "POST",
			headers: {
				"Content-Type": "application/x-www-form-urlencoded",
			},
			body: body.toString(),
			credentials: "include",
		});

		const data = await readJson(response);

		if (!response.ok) {
			const authError = readAuthError(data, "Token refresh failed");
			throw new AuthApiError(
				authError.error_description || authError.error || "Token refresh failed",
				authError.error || "unknown_error",
				response.status,
			);
		}

		if (!isTokenResponse(data)) {
			throw new AuthApiError("Invalid token refresh response", "invalid_response", response.status);
		}

		return data;
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

		const data = await readJson(response);

		if (!response.ok) {
			const authError = readAuthError(data, "Logout failed");
			throw new AuthApiError(
				authError.error_description || authError.error || "Logout failed",
				authError.error || "unknown_error",
				response.status,
			);
		}

		if (!isLogoutResponse(data)) {
			throw new AuthApiError("Invalid logout response", "invalid_response", response.status);
		}

		return data;
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
				const data = await readJson(response);
				const authError = readAuthError(data, "Failed to get user info");
				throw new AuthApiError(
					authError.error_description || authError.error || "Failed to get user info",
					authError.error || "unknown_error",
					response.status,
				);
			}

			const data = await readJson(response);
			return isUserInfo(data) ? data : null;
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
