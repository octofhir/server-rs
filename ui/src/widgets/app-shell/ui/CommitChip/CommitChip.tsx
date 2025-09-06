import { Chip, Loader, Text, Tooltip } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconCopy, IconGitCommit } from "@tabler/icons-react";
import type React from "react";
import { useCallback, useEffect, useState } from "react";
import { serverApi } from "../../../../shared/api";
import type { BuildInfo } from "../../../../shared/api/types";
import styles from "./CommitChip.module.css";

interface CommitChipProps {
  className?: string;
  showIcon?: boolean;
  maxShaLength?: number;
  autoRefresh?: boolean;
  refreshInterval?: number;
  onClick?: (buildInfo: BuildInfo) => void;
}

export const CommitChip: React.FC<CommitChipProps> = ({
  className,
  showIcon = true,
  maxShaLength = 7,
  autoRefresh = false,
  refreshInterval = 300000, // 5 minutes
  onClick,
}) => {
  const [buildInfo, setBuildInfo] = useState<BuildInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Fetch build info
  const fetchBuildInfo = useCallback(async () => {
    try {
      setError(null);
      const info = await serverApi.getBuildInfo();
      setBuildInfo(info);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : "Failed to fetch build info";
      setError(errorMessage);
      console.error("Failed to fetch build info:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  // Copy commit SHA to clipboard
  const handleCopy = useCallback(
    async (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!buildInfo?.commit) {
        notifications.show({
          title: "Copy failed",
          message: "No commit SHA available",
          color: "red",
        });
        return;
      }

      try {
        await navigator.clipboard.writeText(buildInfo.commit);
        notifications.show({
          title: "Copied to clipboard",
          message: `Commit SHA: ${buildInfo.commit}`,
          color: "green",
        });
      } catch (error) {
        // Fallback for browsers without clipboard API
        try {
          const textArea = document.createElement("textarea");
          textArea.value = buildInfo.commit;
          textArea.style.position = "fixed";
          textArea.style.opacity = "0";
          document.body.appendChild(textArea);
          textArea.select();
          document.execCommand("copy");
          document.body.removeChild(textArea);

          notifications.show({
            title: "Copied to clipboard",
            message: `Commit SHA: ${buildInfo.commit}`,
            color: "green",
          });
        } catch (fallbackError) {
          notifications.show({
            title: "Copy failed",
            message: "Unable to copy to clipboard",
            color: "red",
          });
        }
      }
    },
    [buildInfo]
  );

  // Handle chip click
  const handleClick = useCallback(() => {
    if (buildInfo) {
      onClick?.(buildInfo);
    }
  }, [buildInfo, onClick]);

  // Format commit timestamp
  const formatCommitDate = useCallback(() => {
    if (!buildInfo?.commitTimestamp) return "";

    try {
      const date = new Date(buildInfo.commitTimestamp);
      return date.toLocaleDateString("en-US", {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return buildInfo.commitTimestamp;
    }
  }, [buildInfo]);

  // Generate tooltip content
  const getTooltipContent = () => {
    if (loading) {
      return "Loading build information...";
    }

    if (error) {
      return `Error: ${error}`;
    }

    if (!buildInfo) {
      return "No build information available";
    }

    const lines = [`Full SHA: ${buildInfo.commit}`, `Server: ${buildInfo.serverVersion}`];

    if (buildInfo.uiVersion) {
      lines.push(`UI: ${buildInfo.uiVersion}`);
    }

    const commitDate = formatCommitDate();
    if (commitDate) {
      lines.push(`Date: ${commitDate}`);
    }

    lines.push("", "Click to copy full SHA");

    return lines.join("\n");
  };

  // Set up data fetching
  useEffect(() => {
    fetchBuildInfo();

    // Set up auto-refresh if enabled
    let interval: NodeJS.Timeout;
    if (autoRefresh && refreshInterval > 0) {
      interval = setInterval(fetchBuildInfo, refreshInterval);
    }

    return () => {
      if (interval) {
        clearInterval(interval);
      }
    };
  }, [fetchBuildInfo, autoRefresh, refreshInterval]);

  // Loading state
  if (loading) {
    return (
      <div className={`${styles.container} ${className || ""}`}>
        <Tooltip label="Loading build information...">
          <Chip variant="light" color="gray" className={styles.chip} icon={<Loader size="xs" />}>
            <span className={styles.text}>Loading...</span>
          </Chip>
        </Tooltip>
      </div>
    );
  }

  // Error state
  if (error || !buildInfo) {
    return (
      <div className={`${styles.container} ${className || ""}`}>
        <Tooltip label={error || "No build information available"}>
          <Chip variant="light" color="red" className={styles.chip}>
            <Text size="xs" c="dimmed">
              Error
            </Text>
          </Chip>
        </Tooltip>
      </div>
    );
  }

  const shortSha = buildInfo.commit.substring(0, maxShaLength);

  return (
    <div className={`${styles.container} ${className || ""}`}>
      <Tooltip label={getTooltipContent()} position="bottom" multiline className={styles.tooltip}>
        <Chip
          variant="light"
          color="blue"
          className={styles.chip}
          onClick={handleClick}
          icon={showIcon ? <IconGitCommit size={14} /> : undefined}
        >
          <span className={styles.text}>{shortSha}</span>
          <button
            type="button"
            className={styles.copyButton}
            onClick={handleCopy}
            title="Copy full SHA"
            aria-label="Copy full commit SHA"
          >
            <IconCopy size={12} />
          </button>
        </Chip>
      </Tooltip>
    </div>
  );
};
