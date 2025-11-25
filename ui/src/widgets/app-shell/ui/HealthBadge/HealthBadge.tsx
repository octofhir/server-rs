import { type Component, Show, createSignal, createEffect, onMount, onCleanup } from "solid-js";
import { serverApi } from "@/shared/api";
import type { HealthResponse } from "@/shared/api/types";
import { IconCheck, IconAlertTriangle, IconX, IconRefresh } from "@/shared/ui/Icon";
import styles from "./HealthBadge.module.css";

interface HealthBadgeProps {
  pollingInterval?: number;
  maxRetries?: number;
  class?: string;
  showRefreshButton?: boolean;
  onStatusChange?: (status: HealthResponse["status"]) => void;
}

const STATUS_CONFIG = {
  ok: {
    variant: "success" as const,
    Icon: IconCheck,
    label: "Healthy",
  },
  degraded: {
    variant: "warning" as const,
    Icon: IconAlertTriangle,
    label: "Degraded",
  },
  down: {
    variant: "error" as const,
    Icon: IconX,
    label: "Down",
  },
} as const;

export const HealthBadge: Component<HealthBadgeProps> = (props) => {
  const pollingInterval = () => props.pollingInterval ?? 15000;
  const maxRetries = () => props.maxRetries ?? 3;
  const showRefreshButton = () => props.showRefreshButton ?? true;

  const [health, setHealth] = createSignal<HealthResponse | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [lastChecked, setLastChecked] = createSignal<Date | null>(null);
  const [retryCount, setRetryCount] = createSignal(0);
  const [error, setError] = createSignal<string | null>(null);

  const getBackoffDelay = (attempt: number) => {
    return Math.min(1000 * 2 ** attempt, 30000);
  };

  const fetchHealth = async (isRetry = false) => {
    try {
      if (!isRetry) {
        setLoading(true);
        setError(null);
      }

      const healthData = await serverApi.getHealth();
      setHealth(healthData);
      setLastChecked(new Date());
      setRetryCount(0);
      setError(null);
      props.onStatusChange?.(healthData.status);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : "Unknown error";

      if (retryCount() < maxRetries()) {
        const delay = getBackoffDelay(retryCount());
        setRetryCount((prev) => prev + 1);
        setTimeout(() => fetchHealth(true), delay);
      } else {
        const downHealth: HealthResponse = {
          status: "down",
          details: errorMessage,
        };
        setHealth(downHealth);
        setError(errorMessage);
        props.onStatusChange?.(downHealth.status);
      }
      setLastChecked(new Date());
    } finally {
      setLoading(false);
    }
  };

  const handleRefresh = () => {
    setRetryCount(0);
    fetchHealth();
  };

  onMount(() => {
    fetchHealth();

    const interval = setInterval(() => {
      if (!loading()) {
        fetchHealth();
      }
    }, pollingInterval());

    const handleRefreshEvent = () => handleRefresh();
    window.addEventListener("refresh-health", handleRefreshEvent);

    onCleanup(() => {
      clearInterval(interval);
      window.removeEventListener("refresh-health", handleRefreshEvent);
    });
  });

  const formatLastChecked = () => {
    const checked = lastChecked();
    if (!checked) return "Never";

    const now = new Date();
    const diffMs = now.getTime() - checked.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    const diffMins = Math.floor(diffSecs / 60);

    if (diffSecs < 60) return `${diffSecs}s ago`;
    if (diffMins < 60) return `${diffMins}m ago`;
    return checked.toLocaleTimeString();
  };

  const getTooltipContent = () => {
    const lastCheckedText = formatLastChecked();

    if (loading()) return "Checking health status...";
    if (error() && !health()) return `Failed to check health: ${error()}`;

    const currentHealth = health();
    const statusText = currentHealth ? STATUS_CONFIG[currentHealth.status].label : "Unknown";
    const details = currentHealth?.details ? `\nDetails: ${currentHealth.details}` : "";
    const retryText = retryCount() > 0 ? `\nRetrying... (${retryCount()}/${maxRetries()})` : "";

    return `Status: ${statusText}\nLast checked: ${lastCheckedText}${details}${retryText}`;
  };

  const status = () => health()?.status || "down";
  const config = () => STATUS_CONFIG[status()];

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <Show
        when={!loading() || health()}
        fallback={
          <div class={styles.badge} title="Checking health status...">
            <span class={styles.spinner} />
            <span class={styles.label}>Checking...</span>
          </div>
        }
      >
        <button
          class={`${styles.badge} ${styles[status()]}`}
          onClick={handleRefresh}
          title={getTooltipContent()}
        >
          <Show
            when={!loading()}
            fallback={<span class={styles.spinner} />}
          >
            {(() => {
              const Icon = config().Icon;
              return <Icon size={14} />;
            })()}
          </Show>
          <span class={styles.label}>{config().label}</span>
        </button>
      </Show>

      <Show when={showRefreshButton()}>
        <button
          class={styles.refreshButton}
          onClick={handleRefresh}
          disabled={loading()}
          title="Refresh health status"
        >
          <IconRefresh size={12} class={loading() ? styles.spinning : ""} />
        </button>
      </Show>
    </div>
  );
};
