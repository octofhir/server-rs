import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { authApi } from "../authApi";
import {
	clearStoredAuthSession,
	hasStoredRefreshToken,
	refreshAuthSession,
	saveAuthSessionFromToken,
} from "../authSession";

// Query keys for auth
export const authKeys = {
	all: ["auth"] as const,
	user: () => [...authKeys.all, "user"] as const,
};

/**
 * Hook to fetch current user information.
 * Returns null if not authenticated.
 *
 * Automatically refetches every 5 minutes to detect session expiry.
 */
export function useCurrentUser() {
	return useQuery({
		queryKey: authKeys.user(),
		queryFn: async () => {
			const user = await authApi.getCurrentUser();
			if (user) {
				return user;
			}

			if (!hasStoredRefreshToken()) {
				return null;
			}

			const refreshed = await refreshAuthSession(true);
			if (!refreshed) {
				return null;
			}

			return authApi.getCurrentUser();
		},
		retry: false,
		staleTime: 1000 * 60 * 5, // 5 minutes
		refetchInterval: 1000 * 60 * 5, // Refetch every 5 minutes to check session validity
		refetchIntervalInBackground: false, // Don't refetch when tab is not active
		refetchOnWindowFocus: true, // Refetch when user returns to the tab
	});
}

/**
 * Hook for login mutation.
 * On success, refetches user info.
 */
export function useLogin() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: ({ username, password }: { username: string; password: string }) =>
			authApi.login(username, password),
		onSuccess: async (tokenResponse) => {
			saveAuthSessionFromToken(tokenResponse);
			// Refetch user info after successful login
			// Wait for the refetch to complete to ensure auth state is updated
			await queryClient.invalidateQueries({ queryKey: authKeys.user() });
		},
	});
}

/**
 * Hook for logout mutation.
 * On success, clears user info from cache.
 */
export function useLogout() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: () => authApi.logout(),
		onSuccess: () => {
			clearStoredAuthSession();
			// Clear user info and set to null
			queryClient.setQueryData(authKeys.user(), null);
			// Optionally clear all queries on logout
			queryClient.clear();
		},
	});
}

/**
 * Convenience hook that combines user query with auth state.
 */
export function useAuth() {
	const { data: user, isLoading, error, refetch } = useCurrentUser();
	const loginMutation = useLogin();
	const logoutMutation = useLogout();

	return {
		user,
		isAuthenticated: !!user,
		isLoading,
		error,
		refetch,
		login: loginMutation.mutateAsync,
		loginError: loginMutation.error,
		isLoggingIn: loginMutation.isPending,
		logout: logoutMutation.mutateAsync,
		isLoggingOut: logoutMutation.isPending,
	};
}
