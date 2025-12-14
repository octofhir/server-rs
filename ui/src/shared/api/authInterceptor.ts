/**
 * Global authentication interceptor and error handler.
 *
 * This module provides utilities for handling authentication errors globally
 * and triggering logout when sessions expire (401/403 errors).
 */

type AuthErrorCallback = (error: AuthErrorEvent) => void;

export interface AuthErrorEvent {
	status: number;
	statusText: string;
	url: string;
	timestamp: Date;
}

class AuthInterceptor {
	private listeners: Set<AuthErrorCallback> = new Set();
	private lastErrorTime = 0;
	private readonly debounceMs = 1000; // Prevent multiple rapid logouts

	/**
	 * Register a callback to be invoked when an auth error occurs.
	 * Typically used to trigger logout and redirect to login page.
	 */
	onAuthError(callback: AuthErrorCallback): () => void {
		this.listeners.add(callback);

		// Return unsubscribe function
		return () => {
			this.listeners.delete(callback);
		};
	}

	/**
	 * Notify all listeners that an authentication error occurred.
	 * Debounced to prevent multiple rapid logouts from parallel requests.
	 */
	notifyAuthError(event: AuthErrorEvent): void {
		const now = Date.now();

		// Debounce: ignore if a logout was triggered very recently
		if (now - this.lastErrorTime < this.debounceMs) {
			return;
		}

		this.lastErrorTime = now;

		console.warn(
			`[Auth] Session expired or unauthorized: ${event.status} ${event.statusText} at ${event.url}`,
		);

		// Notify all registered listeners
		for (const listener of this.listeners) {
			try {
				listener(event);
			} catch (error) {
				console.error("[Auth] Error in auth error listener:", error);
			}
		}
	}

	/**
	 * Check if a response status code indicates an authentication error.
	 */
	isAuthError(status: number): boolean {
		return status === 401 || status === 403;
	}

	/**
	 * Handle a fetch response and trigger auth error if needed.
	 * Returns true if an auth error was detected.
	 */
	handleResponse(response: Response): boolean {
		if (this.isAuthError(response.status)) {
			this.notifyAuthError({
				status: response.status,
				statusText: response.statusText,
				url: response.url,
				timestamp: new Date(),
			});
			return true;
		}
		return false;
	}
}

// Global singleton instance
export const authInterceptor = new AuthInterceptor();
