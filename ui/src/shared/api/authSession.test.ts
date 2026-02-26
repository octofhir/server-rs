import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuthApiError } from "./authApi";
import {
	clearStoredAuthSession,
	hasStoredRefreshToken,
	refreshAuthSession,
	saveAuthSessionFromToken,
} from "./authSession";

const SESSION_STORAGE_KEY = "octofhir.auth.session";
const { mockRefresh } = vi.hoisted(() => ({
	mockRefresh: vi.fn(),
}));

vi.mock("./authApi", async () => {
	const actual = await vi.importActual<typeof import("./authApi")>("./authApi");
	return {
		...actual,
		authApi: {
			...actual.authApi,
			refresh: mockRefresh,
		},
	};
});

function createSessionStorage(): Storage {
	const storage = new Map<string, string>();

	return {
		get length() {
			return storage.size;
		},
		clear() {
			storage.clear();
		},
		getItem(key: string) {
			return storage.has(key) ? storage.get(key) ?? null : null;
		},
		key(index: number) {
			return Array.from(storage.keys())[index] ?? null;
		},
		removeItem(key: string) {
			storage.delete(key);
		},
		setItem(key: string, value: string) {
			storage.set(key, value);
		},
	};
}

function readStoredSession(): { refreshToken?: string; accessTokenExpiresAt?: number } {
	const raw = window.sessionStorage.getItem(SESSION_STORAGE_KEY);
	if (!raw) {
		return {};
	}
	return JSON.parse(raw) as { refreshToken?: string; accessTokenExpiresAt?: number };
}

describe("authSession", () => {
	beforeEach(() => {
		Object.defineProperty(globalThis, "window", {
			value: {
				sessionStorage: createSessionStorage(),
			},
			configurable: true,
		});
		mockRefresh.mockReset();
	});

	afterEach(() => {
		clearStoredAuthSession();
		Reflect.deleteProperty(globalThis, "window");
	});

	it("stores refresh token and expiry from login response", () => {
		saveAuthSessionFromToken({
			access_token: "access-1",
			token_type: "Bearer",
			expires_in: 60,
			refresh_token: "refresh-1",
		});

		expect(hasStoredRefreshToken()).toBe(true);
		const session = readStoredSession();
		expect(session.refreshToken).toBe("refresh-1");
		expect(session.accessTokenExpiresAt).toBeTypeOf("number");
	});

	it("returns false without stored refresh token", async () => {
		expect(await refreshAuthSession(true)).toBe(false);
		expect(mockRefresh).not.toHaveBeenCalled();
	});

	it("refreshes and keeps previous refresh token when server omits a new one", async () => {
		saveAuthSessionFromToken({
			access_token: "access-1",
			token_type: "Bearer",
			expires_in: 60,
			refresh_token: "refresh-1",
		});
		mockRefresh.mockResolvedValueOnce({
			access_token: "access-2",
			token_type: "Bearer",
			expires_in: 120,
		});

		const refreshed = await refreshAuthSession(true);
		expect(refreshed).toBe(true);
		expect(mockRefresh).toHaveBeenCalledWith("refresh-1");

		const session = readStoredSession();
		expect(session.refreshToken).toBe("refresh-1");
		expect(session.accessTokenExpiresAt).toBeTypeOf("number");
	});

	it("deduplicates concurrent refresh calls (single-flight)", async () => {
		saveAuthSessionFromToken({
			access_token: "access-1",
			token_type: "Bearer",
			expires_in: 60,
			refresh_token: "refresh-1",
		});

		let resolveRefresh: ((value: unknown) => void) | null = null;
		mockRefresh.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					resolveRefresh = resolve;
				}),
		);

		const first = refreshAuthSession(true);
		const second = refreshAuthSession(true);
		expect(mockRefresh).toHaveBeenCalledTimes(1);

		resolveRefresh?.({
			access_token: "access-2",
			token_type: "Bearer",
			expires_in: 60,
			refresh_token: "refresh-2",
		});

		await expect(first).resolves.toBe(true);
		await expect(second).resolves.toBe(true);
	});

	it("clears stored session when refresh token is invalid", async () => {
		saveAuthSessionFromToken({
			access_token: "access-1",
			token_type: "Bearer",
			expires_in: 60,
			refresh_token: "refresh-1",
		});
		mockRefresh.mockRejectedValueOnce(
			new AuthApiError("invalid refresh token", "invalid_grant", 401),
		);

		const refreshed = await refreshAuthSession(true);
		expect(refreshed).toBe(false);
		expect(hasStoredRefreshToken()).toBe(false);
	});
});
