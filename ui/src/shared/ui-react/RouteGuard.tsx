import { Navigate, useLocation } from "react-router-dom";
import { Center, Loader } from "@octofhir/ui-kit";
import { useAuth } from "../api/hooks";

interface RouteGuardProps {
	children: React.ReactNode;
}

/**
 * Route guard component that protects routes requiring authentication.
 * Redirects to login page if not authenticated.
 */
export function RouteGuard({ children }: RouteGuardProps) {
	const { isAuthenticated, isLoading } = useAuth();
	const location = useLocation();

	// Show loading spinner while checking auth status
	if (isLoading) {
		return (
			<Center h="100vh">
				<Loader size="lg" color="primary" />
			</Center>
		);
	}

	// Redirect to login if not authenticated
	if (!isAuthenticated) {
		return <Navigate to="/login" state={{ from: location }} replace />;
	}

	return <>{children}</>;
}

export default RouteGuard;
