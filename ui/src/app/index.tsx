import { Router, Route } from "@solidjs/router";
import { ThemeProvider } from "./providers/ThemeProvider";
import { ToastProvider } from "@/shared/ui/Toast";
import { AppShell } from "@/widgets/app-shell";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { RestConsolePage } from "@/pages/rest-console";
import { SettingsPage } from "@/pages/settings";
import { DashboardPage } from "@/pages/dashboard";
import { DbConsolePage } from "@/pages/db-console";
import { LogsPage } from "@/pages/system-logs";
import { CapabilityStatementPage } from "@/pages/capability-statement";
import { GatewayPage } from "@/pages/gateway";
import { AppDetailPage } from "@/pages/gateway-app";
import { OperationDetailPage } from "@/pages/gateway-operation";

export default function App() {
  return (
    <ThemeProvider>
      <ToastProvider>
        <Router
          base="/ui"
          root={(props) => <AppShell>{props.children}</AppShell>}
        >
          <Route path="/" component={DashboardPage} />
          <Route path="/resources" component={ResourceBrowserPage} />
          <Route path="/resources/:type" component={ResourceBrowserPage} />
          <Route path="/console" component={RestConsolePage} />
          <Route path="/gateway" component={GatewayPage} />
          <Route path="/gateway/apps/:id" component={AppDetailPage} />
          <Route path="/gateway/operations/:id" component={OperationDetailPage} />
          <Route path="/settings" component={SettingsPage} />
          <Route path="/db-console" component={DbConsolePage} />
          <Route path="/logs" component={LogsPage} />
          <Route path="/metadata" component={CapabilityStatementPage} />
        </Router>
      </ToastProvider>
    </ThemeProvider>
  );
}
