import { type Component, Show, createSignal, onMount, onCleanup } from "solid-js";
import { serverApi } from "@/shared/api";
import type { BuildInfo } from "@/shared/api/types";
import { IconGitCommit, IconCopy } from "@/shared/ui/Icon";
import { useToast } from "@/shared/ui/Toast";
import styles from "./CommitChip.module.css";

interface CommitChipProps {
  class?: string;
  showIcon?: boolean;
  maxShaLength?: number;
  autoRefresh?: boolean;
  refreshInterval?: number;
  onClick?: (buildInfo: BuildInfo) => void;
}

export const CommitChip: Component<CommitChipProps> = (props) => {
  const showIcon = () => props.showIcon ?? true;
  const maxShaLength = () => props.maxShaLength ?? 7;
  const autoRefresh = () => props.autoRefresh ?? false;
  const refreshInterval = () => props.refreshInterval ?? 300000;

  const toast = useToast();
  const [buildInfo, setBuildInfo] = createSignal<BuildInfo | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  const fetchBuildInfo = async () => {
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
  };

  const handleCopy = async (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();

    const info = buildInfo();
    if (!info?.commit) {
      toast.error("No commit SHA available", "Copy failed");
      return;
    }

    try {
      await navigator.clipboard.writeText(info.commit);
      toast.success(`Commit SHA: ${info.commit}`, "Copied to clipboard");
    } catch {
      try {
        const textArea = document.createElement("textarea");
        textArea.value = info.commit;
        textArea.style.position = "fixed";
        textArea.style.opacity = "0";
        document.body.appendChild(textArea);
        textArea.select();
        document.execCommand("copy");
        document.body.removeChild(textArea);
        toast.success(`Commit SHA: ${info.commit}`, "Copied to clipboard");
      } catch {
        toast.error("Unable to copy to clipboard", "Copy failed");
      }
    }
  };

  const handleClick = () => {
    const info = buildInfo();
    if (info) {
      props.onClick?.(info);
    }
  };

  const formatCommitDate = () => {
    const info = buildInfo();
    if (!info?.commitTimestamp) return "";

    try {
      const date = new Date(info.commitTimestamp);
      return date.toLocaleDateString("en-US", {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return info.commitTimestamp;
    }
  };

  const getTooltipContent = () => {
    if (loading()) return "Loading build information...";
    if (error()) return `Error: ${error()}`;

    const info = buildInfo();
    if (!info) return "No build information available";

    const lines = [`Full SHA: ${info.commit}`, `Server: ${info.serverVersion}`];
    if (info.uiVersion) lines.push(`UI: ${info.uiVersion}`);

    const commitDate = formatCommitDate();
    if (commitDate) lines.push(`Date: ${commitDate}`);

    lines.push("", "Click to copy full SHA");
    return lines.join("\n");
  };

  onMount(() => {
    fetchBuildInfo();

    let interval: number | undefined;
    if (autoRefresh() && refreshInterval() > 0) {
      interval = window.setInterval(fetchBuildInfo, refreshInterval());
    }

    onCleanup(() => {
      if (interval) clearInterval(interval);
    });
  });

  const shortSha = () => {
    const info = buildInfo();
    return info ? info.commit.substring(0, maxShaLength()) : "";
  };

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <Show
        when={!loading()}
        fallback={
          <div class={`${styles.chip} ${styles.loading}`} title="Loading build information...">
            <span class={styles.spinner} />
            <span class={styles.text}>Loading...</span>
          </div>
        }
      >
        <Show
          when={!error() && buildInfo()}
          fallback={
            <div class={`${styles.chip} ${styles.error}`} title={error() || "No build information available"}>
              <span class={styles.text}>Error</span>
            </div>
          }
        >
          <div
            class={styles.chip}
            onClick={handleClick}
            title={getTooltipContent()}
          >
            <Show when={showIcon()}>
              <IconGitCommit size={14} />
            </Show>
            <span class={styles.text}>{shortSha()}</span>
            <button
              type="button"
              class={styles.copyButton}
              onClick={handleCopy}
              title="Copy full SHA"
              aria-label="Copy full commit SHA"
            >
              <IconCopy size={12} />
            </button>
          </div>
        </Show>
      </Show>
    </div>
  );
};
