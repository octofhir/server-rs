import type { TokenResponse } from "./types";
import { AuthApiError, authApi } from "./authApi";

const SESSION_STORAGE_KEY = "octofhir.auth.session";
const REFRESH_SKEW_MS = 30_000;

interface StoredAuthSession {
	refreshToken?: string;
	accessTokenExpiresAt?: number;
}

let refreshInFlight: Promise<boolean> | null = null;

function canUseStorage(): boolean {
	return typeof window !== "undefined" && typeof window.sessionStorage !== "undefined";
}

function readSession(): StoredAuthSession {
	if (!canUseStorage()) {
		return {};
	}

	const raw = window.sessionStorage.getItem(SESSION_STORAGE_KEY);
	if (!raw) {
		return {};
	}

	try {
		const parsed = JSON.parse(raw) as StoredAuthSession;
		if (typeof parsed !== "object" || parsed === null) {
			return {};
		}
		return parsed;
	} catch {
		return {};
	}
}

function writeSession(session: StoredAuthSession): void {
	if (!canUseStorage()) {
		return;
	}

	window.sessionStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
}

export function saveAuthSessionFromToken(token: TokenResponse): void {
	const current = readSession();
	const next: StoredAuthSession = { ...current };

	if (token.refresh_token && token.refresh_token.trim()) {
		next.refreshToken = token.refresh_token.trim();
	}

	if (Number.isFinite(token.expires_in) && token.expires_in > 0) {
		next.accessTokenExpiresAt = Date.now() + token.expires_in * 1000;
	}

	writeSession(next);
}

export function clearStoredAuthSession(): void {
	if (!canUseStorage()) {
		return;
	}
	window.sessionStorage.removeItem(SESSION_STORAGE_KEY);
}

export function hasStoredRefreshToken(): boolean {
	const refreshToken = readSession().refreshToken;
	return typeof refreshToken === "string" && refreshToken.length > 0;
}

function canSkipRefresh(session: StoredAuthSession): boolean {
	if (!session.accessTokenExpiresAt) {
		return false;
	}
	return Date.now() < session.accessTokenExpiresAt - REFRESH_SKEW_MS;
}

/**
 * Refresh access token using stored refresh token.
 * Uses single-flight to avoid parallel refresh races.
 */
export async function refreshAuthSession(force = true): Promise<boolean> {
	const current = readSession();
	const refreshToken = current.refreshToken;

	if (!refreshToken) {
		return false;
	}

	if (!force && canSkipRefresh(current)) {
		return true;
	}

	if (refreshInFlight) {
		return refreshInFlight;
	}

	refreshInFlight = (async () => {
		try {
			const tokenResponse = await authApi.refresh(refreshToken);
			saveAuthSessionFromToken({
				...tokenResponse,
				refresh_token: tokenResponse.refresh_token ?? refreshToken,
			});
			return true;
		} catch (error) {
			if (
				error instanceof AuthApiError &&
				(error.statusCode === 400 || error.statusCode === 401 || error.statusCode === 403)
			) {
				clearStoredAuthSession();
			}
			return false;
		} finally {
			refreshInFlight = null;
		}
	})();

	return refreshInFlight;
}
