import { type ComponentType, type LazyExoticComponent, lazy, Suspense } from "react";
import { Route, Routes } from "react-router-dom";
import { DashboardPage } from "@/pages/dashboard";
import { LoginPage } from "@/pages/login";
import { RouteGuard } from "@/shared/ui-react/RouteGuard";
import { AppShell } from "@/widgets/app-shell";

/**
 * Pages load on first visit: each is its own chunk, so the console starts with the shell, login
 * and overview only, and editors (Monaco), charts (ECharts) and GraphiQL arrive with the pages
 * that use them. AppShell wraps the outlet in Suspense, so the shell stays put while one loads.
 */
function page<Name extends string>(
  load: () => Promise<Record<Name, ComponentType>>,
  name: Name
): LazyExoticComponent<ComponentType> {
  return lazy(() => load().then((module) => ({ default: module[name] })));
}

const AppsPage = page(() => import("@/pages/apps"), "AppsPage");
const AppDetailPage = page(() => import("@/pages/apps"), "AppDetailPage");
const AuditTrailPage = page(() => import("@/pages/audit"), "AuditTrailPage");
const BulkDataPage = page(() => import("@/pages/bulk-data"), "BulkDataPage");
const AccessPoliciesPage = page(() => import("@/pages/auth"), "AccessPoliciesPage");
const ClientsPage = page(() => import("@/pages/auth"), "ClientsPage");
const IdentityProvidersPage = page(() => import("@/pages/auth"), "IdentityProvidersPage");
const RolesPage = page(() => import("@/pages/auth"), "RolesPage");
const UserDetailPage = page(() => import("@/pages/auth"), "UserDetailPage");
const UsersPage = page(() => import("@/pages/auth"), "UsersPage");
const AutomationsPage = page(() => import("@/pages/automations"), "AutomationsPage");
const AutomationEditorPage = page(() => import("@/pages/automations"), "AutomationEditorPage");
const RestConsolePage = page(() => import("@/pages/console"), "RestConsolePage");
const CqlConsolePage = page(() => import("@/pages/cql-console"), "CqlConsolePage");
const DatabasePage = page(() => import("@/pages/database"), "DatabasePage");
const DbConsolePage = page(() => import("@/pages/db-console"), "DbConsolePage");
const FhirPathConsolePage = page(() => import("@/pages/fhirpath-console"), "FhirPathConsolePage");
const GraphQLConsolePage = page(() => import("@/pages/graphql-console"), "GraphQLConsolePage");
const LogsViewerPage = page(() => import("@/pages/logs"), "LogsViewerPage");
const CapabilityStatementPage = page(() => import("@/pages/metadata"), "CapabilityStatementPage");
const NotebookEditorPage = page(() => import("@/pages/notebooks"), "NotebookEditorPage");
const NotebookListPage = page(() => import("@/pages/notebooks"), "NotebookListPage");
const OperationsPage = page(() => import("@/pages/operations"), "OperationsPage");
const OperationDetailPage = page(() => import("@/pages/operations"), "OperationDetailPage");
const NotFoundPage = page(() => import("@/pages/not-found"), "NotFoundPage");
const PackagesPage = page(() => import("@/pages/packages"), "PackagesPage");
const PackageDetailPage = page(() => import("@/pages/packages"), "PackageDetailPage");
const PerformancePage = page(() => import("@/pages/performance"), "PerformancePage");
const ResourceBrowserPage = page(() => import("@/pages/resource-browser"), "ResourceBrowserPage");
const SessionsPage = page(() => import("@/pages/sessions"), "SessionsPage");
const SettingsPage = page(() => import("@/pages/settings"), "SettingsPage");
const SubscriptionsPage = page(() => import("@/pages/subscriptions"), "SubscriptionsPage");
const TerminologyPage = page(() => import("@/pages/terminology"), "TerminologyPage");
const ValidationPage = page(() => import("@/pages/validation"), "ValidationPage");
const ViewDefinitionPage = page(() => import("@/pages/viewdefinition"), "ViewDefinitionPage");

// Dev-only design specimens; the DEV guard lets the bundler drop the chunk from production builds.
const StyleguidePage = import.meta.env.DEV
  ? lazy(() => import("@/pages/styleguide").then((m) => ({ default: m.StyleguidePage })))
  : null;

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
      {StyleguidePage && (
        <Route
          path="/_styleguide"
          element={
            <Suspense fallback={null}>
              <StyleguidePage />
            </Suspense>
          }
        />
      )}

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
        <Route path="/bulk" element={<BulkDataPage />} />

        {/* Admin */}
        <Route path="/operations" element={<OperationsPage />} />
        <Route path="/operations/:id" element={<OperationDetailPage />} />
        <Route path="/apps" element={<AppsPage />} />
        <Route path="/apps/:id" element={<AppDetailPage />} />
        <Route path="/automations" element={<AutomationsPage />} />
        <Route path="/automations/new" element={<AutomationEditorPage />} />
        <Route path="/automations/:id" element={<AutomationEditorPage />} />
        <Route path="/subscriptions" element={<SubscriptionsPage />} />

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
        <Route path="/notebooks" element={<NotebookListPage />} />
        <Route path="/notebooks/:id" element={<NotebookEditorPage />} />
        <Route path="/settings" element={<SettingsPage />} />

        {/* Other */}
        <Route path="/logs" element={<LogsViewerPage />} />
        <Route path="/performance" element={<PerformancePage />} />
        <Route path="/audit" element={<AuditTrailPage />} />
        <Route path="/metadata" element={<CapabilityStatementPage />} />
        <Route path="/viewdefinition" element={<ViewDefinitionPage />} />
        <Route path="/terminology" element={<TerminologyPage />} />
        <Route path="/validation" element={<ValidationPage />} />

        {/* Any other address: suggests the page or resource it most likely meant */}
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  );
}
