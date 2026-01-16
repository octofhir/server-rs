// Configure Monaco web workers before any Monaco imports
import "@/shared/monaco/config";

import { StrictMode } from "react";
import { BrowserRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MantineProvider } from "@mantine/core";
import { ModalsProvider } from "@mantine/modals";
import { Notifications } from "@mantine/notifications";
import { HelmetProvider } from "react-helmet-async";
import { theme } from "./theme";
import { ThemeCssVars } from "./themeCssVars";
import { AppRoutes } from "./routes";
import { useAuthInterceptor } from "@/shared/api/hooks";
import { ErrorBoundary } from "@/shared/ui";

// Import Mantine styles
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";

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
					<MantineProvider theme={theme} defaultColorScheme="auto">
						<ModalsProvider>
							<ErrorBoundary>
								<ThemeCssVars />
								<Notifications position="top-right" />
								<BrowserRouter basename="/ui">
									<AppContent />
								</BrowserRouter>
							</ErrorBoundary>
						</ModalsProvider>
					</MantineProvider>
				</QueryClientProvider>
			</HelmetProvider>
		</StrictMode>
	);
}

export default App;
