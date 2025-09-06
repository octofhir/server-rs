import { Notifications } from "@mantine/notifications";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { ResourceBrowserPage } from "@/pages/resource-browser";
import { RestConsolePage } from "@/pages/rest-console";
import { SettingsPage } from "@/pages/settings";
import { AppShell } from "@/widgets/app-shell";
import { EffectorProvider, ThemeProvider } from "./providers";

export default function App() {
  return (
    <ThemeProvider>
      <EffectorProvider>
        <BrowserRouter basename="/ui">
          <AppShell>
            <Routes>
              <Route path="/" element={<ResourceBrowserPage />} />
              <Route path="/console" element={<RestConsolePage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Routes>
          </AppShell>
          <Notifications />
        </BrowserRouter>
      </EffectorProvider>
    </ThemeProvider>
  );
}
