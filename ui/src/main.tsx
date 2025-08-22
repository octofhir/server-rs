import { Route, Router } from "@solidjs/router";
import { render } from "solid-js/web";
import HomePage from "@/pages/HomePage";
import ResourceBrowserPage from "@/pages/ResourceBrowserPage";
import RestConsolePage from "@/pages/RestConsolePage";
import SettingsPage from "@/pages/SettingsPage";
import App from "./App";
import "./index.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Root element not found");
}

render(
  () => (
    <Router root={App}>
      <Route path="/" component={HomePage} />
      <Route path="/resources" component={ResourceBrowserPage} />
      <Route path="/console" component={RestConsolePage} />
      <Route path="/settings" component={SettingsPage} />
    </Router>
  ),
  root
);
