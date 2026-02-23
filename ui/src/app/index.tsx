// Configure Monaco web workers before any Monaco imports
import "@/shared/monaco/config";

import { StrictMode } from "react";
import { BrowserRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HelmetProvider } from "react-helmet-async";
import { UIProvider } from "@octofhir/ui-kit";
import { AppRoutes } from "./routes";
import { useAuthInterceptor } from "@/shared/api/hooks";
import { ErrorBoundary } from "@/shared/ui";

// Import existing global styles (CSS variables still work)
import "@/shared/styles/global.css";

// Create QueryClient with sensible defaults
const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			staleTime: 1000 * 60 * 5, // 5 minutes
			gcTime: 1000 * 60 * 30, // 30 minutes (previously cacheTime)
			retry: 1,
			refetchOnWindowFocus: false,
		},
		mutations: {
			retry: 0,
		},
	},
});

/**
 * Inner app wrapper that sets up global auth error handling.
 * Must be inside BrowserRouter to use navigation.
 */
function AppContent() {
	// Set up global auth error interceptor
	useAuthInterceptor();

	return <AppRoutes />;
}

export function App() {
	return (
		<StrictMode>
			<HelmetProvider>
				<QueryClientProvider client={queryClient}>
					<UIProvider>
						<ErrorBoundary>
							<BrowserRouter basename="/ui">
								<AppContent />
							</BrowserRouter>
						</ErrorBoundary>
					</UIProvider>
				</QueryClientProvider>
			</HelmetProvider>
		</StrictMode>
	);
}

export default App;
