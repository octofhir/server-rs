import { Routes, Route } from "react-router-dom";
import { Center, Text, Stack, Title } from "@mantine/core";
import { RouteGuard } from "@/shared/ui-react/RouteGuard";
import { AppShell } from "@/widgets/app-shell";

// Migrated pages
import { LoginPage } from "@/pages/login";
import { DashboardPage } from "@/pages/dashboard";
import { SettingsPage } from "@/pages/settings";
import { DbConsolePage } from "@/pages/db-console";
import { GraphQLConsolePage } from "@/pages/graphql-console";
import { FhirPathConsolePage } from "@/pages/fhirpath-console";
import { CqlConsole } from "@/pages/CqlConsole";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { RestConsolePage } from "@/pages/console";
import { OperationsPage, OperationDetailPage } from "@/pages/operations";
import { AppsPage, AppDetailPage } from "@/pages/apps";
import { ClientsPage, UsersPage, UserDetailPage, RolesPage, AccessPoliciesPage, IdentityProvidersPage } from "@/pages/auth";
import { SessionsPage } from "@/pages/sessions";
import { PackagesPage, PackageDetailPage } from "@/pages/packages";
import { LogsViewerPage } from "@/pages/logs";
import { AuditTrailPage } from "@/pages/audit";
import { ViewDefinitionPage } from "@/pages/viewdefinition";
import { AutomationsPage, AutomationEditorPage } from "@/pages/automations";

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
const CapabilityStatementPage = () => (
  <PlaceholderPage name="Capability Statement" description="FHIR server metadata" />
);

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
			<Route path="/resources/:type/:id" element={<ResourceBrowserPage />} />
			<Route path="/console" element={<RestConsolePage />} />

        {/* Packages */}
        <Route path="/packages" element={<PackagesPage />} />
        <Route path="/packages/:name/:version" element={<PackageDetailPage />} />

        {/* Admin */}
        <Route path="/operations" element={<OperationsPage />} />
        <Route path="/operations/:id" element={<OperationDetailPage />} />
        <Route path="/apps" element={<AppsPage />} />
        <Route path="/apps/:id" element={<AppDetailPage />} />
        <Route path="/automations" element={<AutomationsPage />} />
        <Route path="/automations/new" element={<AutomationEditorPage />} />
        <Route path="/automations/:id" element={<AutomationEditorPage />} />

        {/* Auth */}
        <Route path="/auth/providers" element={<IdentityProvidersPage />} />
        <Route path="/auth/clients" element={<ClientsPage />} />
        <Route path="/auth/users" element={<UsersPage />} />
        <Route path="/auth/users/:id" element={<UserDetailPage />} />
        <Route path="/auth/roles" element={<RolesPage />} />
        <Route path="/auth/policies" element={<AccessPoliciesPage />} />
        <Route path="/auth/sessions" element={<SessionsPage />} />

        {/* Tools */}
        <Route path="/db-console" element={<DbConsolePage />} />
        <Route path="/graphql" element={<GraphQLConsolePage />} />
        <Route path="/fhirpath" element={<FhirPathConsolePage />} />
        <Route path="/cql" element={<CqlConsole />} />
        <Route path="/settings" element={<SettingsPage />} />

        {/* Other */}
        <Route path="/logs" element={<LogsViewerPage />} />
        <Route path="/audit" element={<AuditTrailPage />} />
        <Route path="/metadata" element={<CapabilityStatementPage />} />
        <Route path="/viewdefinition" element={<ViewDefinitionPage />} />
      </Route>
    </Routes>
  );
}
