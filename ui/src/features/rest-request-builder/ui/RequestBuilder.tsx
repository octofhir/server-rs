import { type Component, For, Show, createSignal, createMemo, createEffect, onMount, onCleanup } from "solid-js";
import { useUnit } from "effector-solid";
import type { HttpMethod } from "@/shared/api/types";
import { $apiBaseUrl, $apiTimeout } from "@/entities/settings/model";
import { Input, Select, Button } from "@/shared/ui";
import { IconPlus, IconTrash } from "@/shared/ui/Icon";
import { useToast } from "@/shared/ui/Toast";
import {
  $restRequest,
  removeHeader,
  sendRequestFx,
  setBody,
  setCommonHeader,
  setHeader,
  setMethod,
  setPath,
} from "../model/store";
import { addHistoryItem } from "@/features/rest-console/model/history";
import { setResponseState, setResponseError } from "@/features/rest-response-viewer/model/store";
import styles from "./RequestBuilder.module.css";

const METHOD_OPTIONS: { value: HttpMethod; label: string }[] = [
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
  { value: "DELETE", label: "DELETE" },
  { value: "PATCH", label: "PATCH" },
];

export const RequestBuilder: Component = () => {
  const request = useUnit($restRequest);
  const apiBaseUrl = useUnit($apiBaseUrl);
  const apiTimeout = useUnit($apiTimeout);
  const isSending = useUnit(sendRequestFx.pending);
  const toast = useToast();

  const [newHeaderKey, setNewHeaderKey] = createSignal("");
  const [newHeaderValue, setNewHeaderValue] = createSignal("");

  const onSend = async () => {
    const currentRequest = request();
    const baseUrl = apiBaseUrl();
    const timeout = apiTimeout();

    try {
      setResponseState({ loading: true });
      const result = await sendRequestFx({
        request: currentRequest,
        baseUrl,
        timeout,
      });

      setResponseState({
        loading: false,
        response: result.response,
        durationMs: result.durationMs,
        sizeBytes: result.sizeBytes,
      });

      addHistoryItem({
        id: crypto.randomUUID(),
        timestamp: new Date().toISOString(),
        method: currentRequest.method,
        path: currentRequest.path,
        status: result.response.status,
        duration: result.durationMs,
        success: result.response.status >= 200 && result.response.status < 300,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      setResponseError(message);
      setResponseState({ loading: false });
      toast.error(message, "Request failed");
    }
  };

  // Hotkey: Cmd/Ctrl + Enter to send
  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
        e.preventDefault();
        onSend();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  const headersList = createMemo(() => Object.entries(request().headers));

  const handleAddHeader = () => {
    const key = newHeaderKey();
    if (!key) return;
    setHeader({ key, value: newHeaderValue() });
    setNewHeaderKey("");
    setNewHeaderValue("");
  };

  const handleMethodChange = (e: Event) => {
    const value = (e.target as HTMLSelectElement).value as HttpMethod;
    if (value) setMethod(value);
  };

  return (
    <div class={styles.container}>
      <div class={styles.card}>
        {/* Method and Path */}
        <div class={styles.requestLine}>
          <Select
            value={request().method}
            onChange={handleMethodChange}
            class={styles.methodSelect}
          >
            <For each={METHOD_OPTIONS}>
              {(opt) => <option value={opt.value}>{opt.label}</option>}
            </For>
          </Select>

          <Input
            placeholder="/Patient/123 or /metadata"
            value={request().path}
            onInput={(e) => setPath(e.currentTarget.value)}
            class={styles.pathInput}
          />

          <Button
            onClick={onSend}
            loading={isSending()}
            class={styles.sendButton}
          >
            Send
          </Button>
        </div>

        {/* Headers Section */}
        <div class={styles.section}>
          <div class={styles.sectionHeader}>
            <span class={styles.sectionTitle}>Headers</span>
            <div class={styles.headerQuickActions}>
              <button
                class={styles.quickButton}
                onClick={() => setCommonHeader("Accept")}
                title="Add Accept: application/fhir+json"
              >
                Add Accept
              </button>
              <button
                class={styles.quickButton}
                onClick={() => setCommonHeader("Content-Type")}
                title="Add Content-Type: application/fhir+json"
              >
                Add Content-Type
              </button>
            </div>
          </div>

          <div class={styles.headersList}>
            <Show
              when={headersList().length > 0}
              fallback={<span class={styles.emptyText}>No headers</span>}
            >
              <For each={headersList()}>
                {([key, value]) => (
                  <div class={styles.headerRow}>
                    <Input
                      value={key}
                      readonly
                      class={styles.headerKey}
                    />
                    <Input
                      value={value}
                      onInput={(e) => setHeader({ key, value: e.currentTarget.value })}
                      class={styles.headerValue}
                    />
                    <button
                      class={`${styles.iconButton} ${styles.danger}`}
                      onClick={() => removeHeader(key)}
                      title="Remove header"
                    >
                      <IconTrash size={16} />
                    </button>
                  </div>
                )}
              </For>
            </Show>

            {/* Add new header row */}
            <div class={styles.headerRow}>
              <Input
                placeholder="Header name"
                value={newHeaderKey()}
                onInput={(e) => setNewHeaderKey(e.currentTarget.value)}
                class={styles.headerKey}
              />
              <Input
                placeholder="Header value"
                value={newHeaderValue()}
                onInput={(e) => setNewHeaderValue(e.currentTarget.value)}
                class={styles.headerValue}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleAddHeader();
                }}
              />
              <button
                class={styles.iconButton}
                onClick={handleAddHeader}
                title="Add header"
              >
                <IconPlus size={16} />
              </button>
            </div>
          </div>
        </div>

        {/* Body Section */}
        <div class={styles.section}>
          <label class={styles.sectionTitle}>Body (JSON)</label>
          <textarea
            class={styles.bodyTextarea}
            placeholder="{}"
            value={request().body}
            onInput={(e) => setBody(e.currentTarget.value)}
            rows={6}
          />
        </div>

        {/* Keyboard shortcut hint */}
        <div class={styles.hint}>
          Press <kbd>Cmd/Ctrl + Enter</kbd> to send request
        </div>
      </div>
    </div>
  );
};
