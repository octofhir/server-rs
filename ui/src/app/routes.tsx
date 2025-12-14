import { Routes, Route } from "react-router-dom";
import { Center, Text, Stack, Title } from "@mantine/core";
import { RouteGuard } from "@/shared/ui-react/RouteGuard";
import { AppShell } from "@/widgets/app-shell-react";

// Migrated pages
import { LoginPage } from "@/pages/login";
import { DashboardPage } from "@/pages/dashboard";
import { SettingsPage } from "@/pages/settings";
import { DbConsolePage } from "@/pages/db-console";
import { GraphQLConsolePage } from "@/pages/graphql-console";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { RestConsolePage } from "@/pages/rest-console";
import { OperationsPage, OperationDetailPage } from "@/pages/operations";
import { AppsPage } from "@/pages/apps";
import { ClientsPage, UsersPage, AccessPoliciesPage } from "@/pages/auth";

// Placeholder component for pages during migration
function PlaceholderPage({ name, description }: { name: string; description?: string }) {
	return (
		<Center h="100%">
			<Stack align="center" gap="md">
				<Title order={2}>{name}</Title>
				<Text c="dimmed">{description || "Migration in progress"}</Text>
			</Stack>
		</Center>
	);
}

// Placeholder pages (will be replaced with actual components)
const LogsPage = () => <PlaceholderPage name="System Logs" description="View server activity logs" />;
const CapabilityStatementPage = () => <PlaceholderPage name="Capability Statement" description="FHIR server metadata" />;

// Protected layout with RouteGuard and AppShell
function ProtectedLayout() {
	return (
		<RouteGuard>
			<AppShell />
		</RouteGuard>
	);
}

export function AppRoutes() {
	return (
		<Routes>
			{/* Public routes */}
			<Route path="/login" element={<LoginPage />} />

			{/* Protected routes - require authentication */}
			<Route element={<ProtectedLayout />}>
				{/* Main */}
				<Route index element={<DashboardPage />} />
				<Route path="/resources" element={<ResourceBrowserPage />} />
				<Route path="/resources/:type" element={<ResourceBrowserPage />} />
				<Route path="/console" element={<RestConsolePage />} />

				{/* Admin */}
				<Route path="/operations" element={<OperationsPage />} />
				<Route path="/operations/:id" element={<OperationDetailPage />} />
				<Route path="/apps" element={<AppsPage />} />

				{/* Auth */}
				<Route path="/auth/clients" element={<ClientsPage />} />
				<Route path="/auth/users" element={<UsersPage />} />
				<Route path="/auth/policies" element={<AccessPoliciesPage />} />

				{/* Tools */}
				<Route path="/db-console" element={<DbConsolePage />} />
				<Route path="/graphql" element={<GraphQLConsolePage />} />
				<Route path="/settings" element={<SettingsPage />} />

				{/* Other */}
				<Route path="/logs" element={<LogsPage />} />
				<Route path="/metadata" element={<CapabilityStatementPage />} />
			</Route>
		</Routes>
	);
}
