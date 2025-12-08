import { type ParentComponent, onMount, createEffect, Show } from "solid-js";
import { useNavigate, useLocation } from "@solidjs/router";
import { Loader } from "../Loader";
import { isAuthenticated, isLoading, checkAuth } from "@/entities/auth";

export interface RouteGuardProps {
  /**
   * Route to redirect to if not authenticated.
   * @default "/login"
   */
  loginPath?: string;
}

/**
 * Route guard component that protects routes from unauthenticated access.
 *
 * Usage:
 * ```tsx
 * <RouteGuard>
 *   <ProtectedContent />
 * </RouteGuard>
 * ```
 */
export const RouteGuard: ParentComponent<RouteGuardProps> = (props) => {
  const navigate = useNavigate();
  const location = useLocation();
  const loginPath = () => props.loginPath ?? "/login";

  onMount(async () => {
    // Check auth status on mount
    await checkAuth();
  });

  // Reactive effect to handle redirect when not authenticated
  createEffect(() => {
    const loading = isLoading();
    const authenticated = isAuthenticated();

    // Only redirect after loading completes and user is not authenticated
    if (!loading && !authenticated) {
      // Save the attempted URL for redirect after login
      const returnUrl = location.pathname + location.search;
      navigate(`${loginPath()}?returnUrl=${encodeURIComponent(returnUrl)}`, { replace: true });
    }
  });

  return (
    <Show
      when={!isLoading()}
      fallback={<Loader fullScreen label="Checking authentication..." />}
    >
      <Show when={isAuthenticated()}>
        {props.children}
      </Show>
    </Show>
  );
};
