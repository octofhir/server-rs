import { ActionIcon, Badge, Loader, Tooltip } from "@mantine/core";
import { IconAlertTriangle, IconCircleCheck, IconCircleX, IconRefresh } from "@tabler/icons-react";
import type React from "react";
import { useCallback, useEffect, useState } from "react";
import { serverApi } from "../../../../shared/api";
import type { HealthResponse } from "../../../../shared/api/types";
import styles from "./HealthBadge.module.css";

interface HealthBadgeProps {
  pollingInterval?: number;
  maxRetries?: number;
  className?: string;
  showRefreshButton?: boolean;
  onStatusChange?: (status: HealthResponse["status"]) => void;
}

const STATUS_CONFIG = {
  ok: {
    color: "green",
    icon: IconCircleCheck,
    label: "Healthy",
  },
  degraded: {
    color: "yellow",
    icon: IconAlertTriangle,
    label: "Degraded",
  },
  down: {
    color: "red",
    icon: IconCircleX,
    label: "Down",
  },
} as const;

export const HealthBadge: React.FC<HealthBadgeProps> = ({
  pollingInterval = 15000, // 15 seconds
  maxRetries = 3,
  className,
  showRefreshButton = true,
  onStatusChange,
}) => {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastChecked, setLastChecked] = useState<Date | null>(null);
  const [retryCount, setRetryCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Calculate backoff delay for retries
  const getBackoffDelay = useCallback((attempt: number) => {
    return Math.min(1000 * 2 ** attempt, 30000); // Max 30s
  }, []);

  // Fetch health status
  const fetchHealth = useCallback(
    async (isRetry = false) => {
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

        // Notify about status change
        onStatusChange?.(healthData.status);
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : "Unknown error";

        // If we haven't reached max retries, schedule a retry with backoff
        if (retryCount < maxRetries) {
          const delay = getBackoffDelay(retryCount);
          setRetryCount((prev) => prev + 1);

          setTimeout(() => {
            fetchHealth(true);
          }, delay);
        } else {
          // Max retries reached, set to down status
          const downHealth: HealthResponse = {
            status: "down",
            details: errorMessage,
          };

          setHealth(downHealth);
          setError(errorMessage);
          onStatusChange?.(downHealth.status);
        }

        setLastChecked(new Date());
      } finally {
        setLoading(false);
      }
    },
    [retryCount, maxRetries, getBackoffDelay, onStatusChange]
  );

  // Manual refresh
  const handleRefresh = useCallback(() => {
    setRetryCount(0); // Reset retry count for manual refresh
    fetchHealth();
  }, [fetchHealth]);

  // Set up polling
  useEffect(() => {
    // Initial fetch
    fetchHealth();

    // Set up polling interval
    const interval = setInterval(() => {
      if (!loading) {
        fetchHealth();
      }
    }, pollingInterval);

    // Listen for refresh events from command palette
    const handleRefreshEvent = () => handleRefresh();
    window.addEventListener("refresh-health", handleRefreshEvent);

    return () => {
      clearInterval(interval);
      window.removeEventListener("refresh-health", handleRefreshEvent);
    };
  }, [fetchHealth, handleRefresh, pollingInterval, loading]);

  // Format last checked time
  const formatLastChecked = useCallback(() => {
    if (!lastChecked) return "Never";

    const now = new Date();
    const diffMs = now.getTime() - lastChecked.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    const diffMins = Math.floor(diffSecs / 60);

    if (diffSecs < 60) {
      return `${diffSecs}s ago`;
    } else if (diffMins < 60) {
      return `${diffMins}m ago`;
    } else {
      return lastChecked.toLocaleTimeString();
    }
  }, [lastChecked]);

  // Generate tooltip content
  const getTooltipContent = () => {
    const lastCheckedText = formatLastChecked();

    if (loading) {
      return "Checking health status...";
    }

    if (error && !health) {
      return `Failed to check health: ${error}`;
    }

    const statusText = health ? STATUS_CONFIG[health.status].label : "Unknown";
    const details = health?.details ? `\nDetails: ${health.details}` : "";
    const retryText = retryCount > 0 ? `\nRetrying... (${retryCount}/${maxRetries})` : "";

    return `Status: ${statusText}\nLast checked: ${lastCheckedText}${details}${retryText}`;
  };

  if (loading && !health) {
    return (
      <div className={`${styles.container} ${className || ""}`}>
        <Tooltip label="Checking health status...">
          <Badge color="gray" variant="dot" className={styles.badge}>
            <Loader size="xs" />
            <span className={styles.label}>Checking...</span>
          </Badge>
        </Tooltip>
      </div>
    );
  }

  const status = health?.status || "down";
  const config = STATUS_CONFIG[status];
  const Icon = config.icon;

  return (
    <div className={`${styles.container} ${className || ""}`}>
      <Tooltip label={getTooltipContent()} position="bottom" multiline className={styles.tooltip}>
        <Badge
          color={config.color}
          variant="dot"
          className={`${styles.badge} ${styles[status]}`}
          onClick={handleRefresh}
          style={{ cursor: "pointer" }}
        >
          {loading ? <Loader size="xs" /> : <Icon size={14} />}
          <span className={styles.label}>{config.label}</span>
        </Badge>
      </Tooltip>

      {showRefreshButton && (
        <Tooltip label="Refresh health status">
          <ActionIcon
            size="sm"
            variant="subtle"
            color="gray"
            onClick={handleRefresh}
            loading={loading}
            className={styles.refreshButton}
          >
            <IconRefresh size={12} />
          </ActionIcon>
        </Tooltip>
      )}
    </div>
  );
};
