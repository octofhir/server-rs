import { onMount, createSignal, Show, For } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { Card, Loader, Button } from "@/shared/ui";
import {
  loadCapabilities,
  capabilitiesLoading,
  getResourceTypes,
} from "@/entities/fhir";
import {
  connectionStatus,
  startHealthPolling,
  stopHealthPolling,
} from "@/entities/system";
import styles from "./DashboardPage.module.css";

interface QuickAction {
  title: string;
  description: string;
  href: string;
  icon: string;
}

const quickActions: QuickAction[] = [
  {
    title: "Browse Resources",
    description: "View and search FHIR resources",
    href: "/resources",
    icon: "📋",
  },
  {
    title: "REST Console",
    description: "Test FHIR API endpoints",
    href: "/console",
    icon: "🔧",
  },
  {
    title: "DB Console",
    description: "Execute SQL queries",
    href: "/db-console",
    icon: "💾",
  },
  {
    title: "System Logs",
    description: "View server activity logs",
    href: "/logs",
    icon: "📊",
  },
  {
    title: "Capability Statement",
    description: "View server metadata",
    href: "/metadata",
    icon: "📄",
  },
  {
    title: "Settings",
    description: "Configure server settings",
    href: "/settings",
    icon: "⚙️",
  },
];

export const DashboardPage = () => {
  const [resourceCount, setResourceCount] = createSignal(0);
  const navigate = useNavigate();

  onMount(async () => {
    startHealthPolling();
    try {
      await loadCapabilities();
      setResourceCount(getResourceTypes().length);
    } catch (err) {
      console.error("Failed to load capabilities:", err);
    }

    return () => stopHealthPolling();
  });

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <h1 class={styles.title}>Dashboard</h1>
        <p class={styles.subtitle}>Welcome to OctoFHIR Server UI</p>
      </div>

      <div class={styles.statsGrid}>
        <Card class={styles.statCard} padding="md">
          <div class={styles.statHeader}>
            <span class={styles.statIcon}>🟢</span>
            <h3 class={styles.statTitle}>Server Status</h3>
          </div>
          <div class={styles.statContent}>
            <div
              class={styles.statusIndicator}
              classList={{
                [styles.connected]: connectionStatus() === "connected",
                [styles.connecting]: connectionStatus() === "connecting",
                [styles.disconnected]: connectionStatus() === "disconnected",
              }}
            />
            <span class={styles.statusText}>
              {connectionStatus() === "connected"
                ? "Connected"
                : connectionStatus() === "connecting"
                  ? "Connecting..."
                  : "Disconnected"}
            </span>
          </div>
        </Card>

        <Card class={styles.statCard} padding="md">
          <div class={styles.statHeader}>
            <span class={styles.statIcon}>📦</span>
            <h3 class={styles.statTitle}>Resource Types</h3>
          </div>
          <div class={styles.statContent}>
            <Show when={!capabilitiesLoading()} fallback={<Loader size="sm" />}>
              <div class={styles.statValue}>
                <span class={styles.count}>{resourceCount()}</span>
                <span class={styles.countLabel}>available types</span>
              </div>
            </Show>
          </div>
        </Card>
      </div>

      <div class={styles.section}>
        <h2 class={styles.sectionTitle}>Quick Actions</h2>
        <div class={styles.actionsGrid}>
          <For each={quickActions}>
            {(action) => (
              <Card class={styles.actionCard} padding="md">
                <div class={styles.actionIcon}>{action.icon}</div>
                <h3 class={styles.actionTitle}>{action.title}</h3>
                <p class={styles.actionDescription}>{action.description}</p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => navigate(action.href)}
                  class={styles.actionButton}
                >
                  Open
                </Button>
              </Card>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};
