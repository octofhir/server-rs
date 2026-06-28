import { Route, Routes } from "react-router-dom";
import { AppDetailPage, AppsPage } from "@/pages/apps";
import { AuditTrailPage } from "@/pages/audit";
import {
  AccessPoliciesPage,
  ClientsPage,
  IdentityProvidersPage,
  RolesPage,
  UserDetailPage,
  UsersPage,
} from "@/pages/auth";
import { AutomationEditorPage, AutomationsPage } from "@/pages/automations";
import { RestConsolePage } from "@/pages/console";
import { CqlConsolePage } from "@/pages/cql-console";
import { DashboardPage } from "@/pages/dashboard";
import { DatabasePage } from "@/pages/database";
import { DbConsolePage } from "@/pages/db-console";
import { FhirPathConsolePage } from "@/pages/fhirpath-console";
import { GraphQLConsolePage } from "@/pages/graphql-console";
// Migrated pages
import { LoginPage } from "@/pages/login";
import { LogsViewerPage } from "@/pages/logs";
import { CapabilityStatementPage } from "@/pages/metadata";
import { OperationDetailPage, OperationsPage } from "@/pages/operations";
import { PackageDetailPage, PackagesPage } from "@/pages/packages";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { SessionsPage } from "@/pages/sessions";
import { SettingsPage } from "@/pages/settings";
import { ViewDefinitionPage } from "@/pages/viewdefinition";
import { RouteGuard } from "@/shared/ui-react/RouteGuard";
import { AppShell } from "@/widgets/app-shell";

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
        <Route path="/database" element={<DatabasePage />} />
        <Route path="/db-console" element={<DbConsolePage />} />
        <Route path="/graphql" element={<GraphQLConsolePage />} />
        <Route path="/fhirpath" element={<FhirPathConsolePage />} />
        <Route path="/cql" element={<CqlConsolePage />} />
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
