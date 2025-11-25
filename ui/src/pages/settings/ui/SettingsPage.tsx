import { createSignal } from "solid-js";
import { Button, Card, Input, Select } from "@/shared/ui";
import {
  fhirBaseUrl,
  setFhirBaseUrl,
  requestTimeout,
  setRequestTimeout,
  colorScheme,
  setColorScheme,
} from "@/entities/settings";
import { connectionStatus, checkHealth } from "@/entities/system";
import styles from "./SettingsPage.module.css";

const themeOptions = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "auto", label: "System" },
];

export const SettingsPage = () => {
  const [healthLoading, setHealthLoading] = createSignal(false);

  const handleTestConnection = async () => {
    setHealthLoading(true);
    try {
      await checkHealth();
    } finally {
      setHealthLoading(false);
    }
  };

  const handleTimeoutChange = (e: Event) => {
    const target = e.currentTarget as HTMLInputElement;
    const value = parseInt(target.value, 10);
    if (!Number.isNaN(value) && value >= 1000) {
      setRequestTimeout(value);
    }
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <h1 class={styles.title}>Settings</h1>
        <p class={styles.subtitle}>Configure server settings and preferences</p>
      </div>

      <div class={styles.section}>
        <Card>
          <div class={styles.sectionHeader}>
            <h3>Connection</h3>
            <div class={styles.statusGroup}>
              <span
                class={styles.statusIndicator}
                classList={{
                  [styles.connected]: connectionStatus() === "connected",
                  [styles.connecting]: connectionStatus() === "connecting",
                  [styles.disconnected]: connectionStatus() === "disconnected",
                }}
              />
              <span class={styles.statusText}>{connectionStatus()}</span>
              <Button size="sm" onClick={handleTestConnection} loading={healthLoading()}>
                Test connection
              </Button>
            </div>
          </div>

          <div class={styles.formGroup}>
            <Input
              label="FHIR Base URL"
              value={fhirBaseUrl()}
              onInput={(e) => setFhirBaseUrl(e.currentTarget.value)}
              placeholder="http://localhost:8080"
              fullWidth
            />
            <span class={styles.helpText}>
              Base URL of your FHIR server (e.g., http://localhost:8080)
            </span>
          </div>

          <div class={styles.formGroup}>
            <Input
              label="Request timeout (ms)"
              type="number"
              value={requestTimeout().toString()}
              onInput={handleTimeoutChange}
              min={1000}
              step={500}
              fullWidth
            />
            <span class={styles.helpText}>
              How long to wait before a request is aborted
            </span>
          </div>
        </Card>
      </div>

      <div class={styles.section}>
        <Card>
          <div class={styles.sectionHeader}>
            <h3>Appearance</h3>
          </div>

          <div class={styles.formGroup}>
            <Select
              label="Theme"
              options={themeOptions}
              value={colorScheme()}
              onChange={(v) => setColorScheme(v as "light" | "dark" | "auto")}
            />
          </div>
        </Card>
      </div>
    </div>
  );
};
