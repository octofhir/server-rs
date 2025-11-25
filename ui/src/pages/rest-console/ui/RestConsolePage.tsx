import { createSignal, Show } from "solid-js";
import { Button, Card, Input, Select, JsonViewer, Loader } from "@/shared/ui";
import { fhirClient } from "@/shared/api";
import styles from "./RestConsolePage.module.css";

type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";

const methodOptions = [
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
  { value: "DELETE", label: "DELETE" },
  { value: "PATCH", label: "PATCH" },
];

export const RestConsolePage = () => {
  const [method, setMethod] = createSignal<HttpMethod>("GET");
  const [path, setPath] = createSignal("/Patient");
  const [requestBody, setRequestBody] = createSignal("");
  const [response, setResponse] = createSignal<unknown>(null);
  const [responseStatus, setResponseStatus] = createSignal<number | null>(null);
  const [responseTime, setResponseTime] = createSignal<number | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleSubmit = async () => {
    setLoading(true);
    setError(null);
    setResponse(null);
    setResponseStatus(null);
    setResponseTime(null);

    try {
      let body: unknown = undefined;
      if (requestBody() && ["POST", "PUT", "PATCH"].includes(method())) {
        body = JSON.parse(requestBody());
      }

      const result = await fhirClient.rawRequest(method(), path(), body);
      setResponse(result.data);
      setResponseStatus(result.status);
      setResponseTime(Math.round(result.responseTime));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Request failed");
    } finally {
      setLoading(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      handleSubmit();
    }
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <h1 class={styles.title}>REST Console</h1>
        <p class={styles.subtitle}>Test and interact with FHIR REST API endpoints</p>
      </div>

      <Card class={styles.requestCard}>
        <div class={styles.requestForm}>
          <div class={styles.methodSelect}>
            <Select
              label="Method"
              options={methodOptions}
              value={method()}
              onChange={(v) => setMethod(v as HttpMethod)}
            />
          </div>
          <div class={styles.pathInput}>
            <Input
              label="Path"
              value={path()}
              onInput={(e) => setPath(e.currentTarget.value)}
              onKeyDown={handleKeyDown}
              placeholder="/Patient"
              fullWidth
            />
          </div>
          <Button onClick={handleSubmit} loading={loading()}>
            Send
          </Button>
        </div>

        <Show when={["POST", "PUT", "PATCH"].includes(method())}>
          <div class={styles.bodySection}>
            <label class={styles.bodyLabel}>Request Body (JSON)</label>
            <textarea
              class={styles.bodyTextarea}
              value={requestBody()}
              onInput={(e) => setRequestBody(e.currentTarget.value)}
              placeholder='{"resourceType": "Patient", ...}'
              rows={8}
            />
          </div>
        </Show>
      </Card>

      <Card class={styles.responseCard}>
        <div class={styles.responseHeader}>
          <h3>Response</h3>
          <Show when={responseStatus() !== null}>
            <div class={styles.responseMeta}>
              <span
                class={styles.statusBadge}
                classList={{
                  [styles.success]: responseStatus()! < 300,
                  [styles.error]: responseStatus()! >= 400,
                }}
              >
                {responseStatus()}
              </span>
              <Show when={responseTime() !== null}>
                <span class={styles.responseTime}>{responseTime()}ms</span>
              </Show>
            </div>
          </Show>
        </div>

        <Show when={loading()}>
          <div class={styles.loaderContainer}>
            <Loader label="Sending request..." />
          </div>
        </Show>

        <Show when={error()}>
          <div class={styles.errorMessage}>{error()}</div>
        </Show>

        <Show when={response()}>
          <JsonViewer data={response()} />
        </Show>

        <Show when={!loading() && !error() && !response()}>
          <div class={styles.emptyState}>Send a request to see the response</div>
        </Show>
      </Card>
    </div>
  );
};
