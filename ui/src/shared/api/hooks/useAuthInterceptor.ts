import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { authInterceptor } from "../authInterceptor";
import { authKeys } from "./useAuth";
import { useUiSettings } from "@/shared";

/**
 * Hook that sets up global auth error handling.
 * Automatically logs out and redirects to login when 401/403 errors occur.
 *
 * This should be called once at the app root level.
 */
export function useAuthInterceptor() {
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const [settings] = useUiSettings();

	useEffect(() => {
		if (settings.disableAuthAutoLogout) {
			return;
		}

		// Register auth error handler
		const unsubscribe = authInterceptor.onAuthError((event) => {
			console.log(
				`[Auth] Session expired (${event.status}), logging out and redirecting to login`,
			);

			// Clear user data from cache
			queryClient.setQueryData(authKeys.user(), null);

			// Clear all queries to reset app state
			queryClient.clear();

			// Redirect to login page with return path
			navigate("/login", {
				replace: true,
				state: { from: { pathname: window.location.pathname } },
			});
		});

		// Cleanup on unmount
		return unsubscribe;
	}, [navigate, queryClient, settings.disableAuthAutoLogout]);
}
