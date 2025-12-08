import { createSignal } from "solid-js";
import { authApi, type UserInfo } from "@/shared/api";

// Auth state
const [user, setUser] = createSignal<UserInfo | null>(null);
const [isAuthenticated, setIsAuthenticated] = createSignal(false);
const [isLoading, setIsLoading] = createSignal(true); // Start as loading to check auth
const [authError, setAuthError] = createSignal<string | null>(null);

// Computed
const isAdmin = () => {
  const currentUser = user();
  return currentUser?.roles?.includes("admin") ?? false;
};

// Actions

/**
 * Login with username and password.
 * On success, sets user state and marks as authenticated.
 */
export const login = async (username: string, password: string): Promise<void> => {
  setIsLoading(true);
  setAuthError(null);

  try {
    // Call token endpoint - cookie is set automatically
    await authApi.login(username, password);

    // Fetch user info after successful login
    const userInfo = await authApi.getCurrentUser();
    if (userInfo) {
      setUser(userInfo);
      setIsAuthenticated(true);
    } else {
      throw new Error("Failed to get user info after login");
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : "Login failed";
    setAuthError(message);
    setIsAuthenticated(false);
    setUser(null);
    throw err;
  } finally {
    setIsLoading(false);
  }
};

/**
 * Logout the current user.
 * Clears user state and marks as unauthenticated.
 */
export const logout = async (): Promise<void> => {
  setIsLoading(true);
  setAuthError(null);

  try {
    await authApi.logout();
  } catch (err) {
    // Log but don't fail - we still want to clear local state
    console.warn("Logout request failed:", err);
  } finally {
    setUser(null);
    setIsAuthenticated(false);
    setIsLoading(false);
  }
};

/**
 * Check if the current session is authenticated.
 * Called on app startup to restore session from cookie.
 */
export const checkAuth = async (): Promise<boolean> => {
  setIsLoading(true);
  setAuthError(null);

  try {
    const userInfo = await authApi.getCurrentUser();
    if (userInfo) {
      setUser(userInfo);
      setIsAuthenticated(true);
      return true;
    }
    setUser(null);
    setIsAuthenticated(false);
    return false;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Auth check failed";
    setAuthError(message);
    setUser(null);
    setIsAuthenticated(false);
    return false;
  } finally {
    setIsLoading(false);
  }
};

/**
 * Clear any auth error.
 */
export const clearAuthError = () => {
  setAuthError(null);
};

// Exports
export {
  user,
  isAuthenticated,
  isLoading,
  authError,
  isAdmin,
};
