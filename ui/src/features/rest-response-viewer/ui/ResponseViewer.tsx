import { type Component, For, Show, createSignal, createMemo } from "solid-js";
import { useUnit } from "effector-solid";
import { JsonViewer } from "@/shared/ui";
import { $responseState } from "../model/store";
import styles from "./ResponseViewer.module.css";

function statusVariant(status?: number): "success" | "warning" | "error" | "neutral" {
  if (!status) return "neutral";
  if (status >= 200 && status < 300) return "success";
  if (status >= 300 && status < 400) return "warning";
  if (status >= 400) return "error";
  return "neutral";
}

export const ResponseViewer: Component = () => {
  const state = useUnit($responseState);
  const [showRaw, setShowRaw] = createSignal(false);

  const headersList = createMemo(() =>
    Object.entries(state().response?.headers ?? {})
  );

  const dataForViewer = createMemo(() => {
    const response = state().response;
    if (!response) return null;
    const data = response.data;
    if (showRaw()) {
      return typeof data === "string" ? data : JSON.stringify(data, null, 2);
    }
    return data;
  });

  const formatBytes = (bytes: number | null): string => {
    if (bytes === null) return "-";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div class={styles.container}>
      <div class={styles.card}>
        {/* Header */}
        <div class={styles.header}>
          <div class={styles.headerLeft}>
            <span class={styles.title}>Response</span>
            <Show when={state().response}>
              <span class={`${styles.badge} ${styles[statusVariant(state().response?.status)]}`}>
                {state().response?.status} {state().response?.statusText}
              </span>
            </Show>
          </div>
          <div class={styles.headerRight}>
            <span class={styles.stat}>
              {state().durationMs != null ? `${state().durationMs} ms` : "-"}
            </span>
            <span class={styles.stat}>
              {formatBytes(state().sizeBytes)}
            </span>
            <button
              class={`${styles.toggleButton} ${showRaw() ? styles.active : ""}`}
              onClick={() => setShowRaw(!showRaw())}
            >
              {showRaw() ? "Raw" : "JSON"}
            </button>
          </div>
        </div>

        {/* Loading State */}
        <Show when={state().loading}>
          <div class={styles.loadingState}>
            <span class={styles.loadingText}>Sending request...</span>
          </div>
        </Show>

        {/* Error State */}
        <Show when={state().error}>
          <div class={styles.errorState}>
            <span class={styles.errorText}>{state().error}</span>
          </div>
        </Show>

        {/* Headers Section */}
        <Show when={headersList().length > 0}>
          <div class={styles.section}>
            <span class={styles.sectionTitle}>Headers</span>
            <div class={styles.headersContainer}>
              <For each={headersList()}>
                {([key, value]) => (
                  <div class={styles.headerRow}>
                    <span class={styles.headerKey} title={key}>{key}</span>
                    <span class={styles.headerValue} title={String(value)}>{String(value)}</span>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>

        {/* Body Section */}
        <Show when={state().response}>
          <div class={styles.section}>
            <span class={styles.sectionTitle}>Body</span>
            <div class={styles.bodyContainer}>
              <Show
                when={typeof dataForViewer() !== "string"}
                fallback={
                  <pre class={styles.rawContent}>{dataForViewer() as string}</pre>
                }
              >
                <JsonViewer data={dataForViewer()} />
              </Show>
            </div>
          </div>
        </Show>

        {/* Empty State */}
        <Show when={!state().response && !state().loading && !state().error}>
          <div class={styles.emptyState}>
            <span class={styles.emptyText}>Send a request to see the response</span>
          </div>
        </Show>
      </div>
    </div>
  );
};
