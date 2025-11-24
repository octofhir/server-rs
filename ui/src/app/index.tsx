import { Router, Route } from "@solidjs/router";
import { ThemeProvider } from "./providers/ThemeProvider";
import { AppShell } from "@/widgets/app-shell";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { RestConsolePage } from "@/pages/rest-console";
import { SettingsPage } from "@/pages/settings";
import { DashboardPage } from "@/pages/dashboard";
import { DbConsolePage } from "@/pages/db-console";
import { LogsPage } from "@/pages/system-logs";
import { CapabilityStatementPage } from "@/pages/capability-statement";

export default function App() {
  return (
    <ThemeProvider>
      <Router base="/ui">
        <Route path="/" component={() => (
          <AppShell>
            <DashboardPage />
          </AppShell>
        )} />
        <Route path="/resources" component={() => (
          <AppShell>
            <ResourceBrowserPage />
          </AppShell>
        )} />
        <Route path="/resources/:type" component={() => (
          <AppShell>
            <ResourceBrowserPage />
          </AppShell>
        )} />
        <Route path="/console" component={() => (
          <AppShell>
            <RestConsolePage />
          </AppShell>
        )} />
        <Route path="/settings" component={() => (
          <AppShell>
            <SettingsPage />
          </AppShell>
        )} />
        <Route path="/db-console" component={() => (
          <AppShell>
            <DbConsolePage />
          </AppShell>
        )} />
        <Route path="/logs" component={() => (
          <AppShell>
            <LogsPage />
          </AppShell>
        )} />
        <Route path="/metadata" component={() => (
          <AppShell>
            <CapabilityStatementPage />
          </AppShell>
        )} />
      </Router>
    </ThemeProvider>
  );
}
