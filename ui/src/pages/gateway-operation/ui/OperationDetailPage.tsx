import { createSignal, onMount, Show, For } from "solid-js";
import { useParams, useNavigate, useSearchParams } from "@solidjs/router";
import {
  Button,
  Card,
  Input,
  Loader,
  Select,
} from "@/shared/ui";
import {
  loadOperation,
  createOperation,
  updateOperation,
  loadApps,
  apps,
  selectedOperation,
  operationsLoading,
  operationsError,
  type CustomOperation,
  type OperationType,
  type HttpMethod,
  type ProxyConfig,
  type SqlConfig,
  type FhirPathConfig,
  type HandlerConfig,
} from "@/entities/gateway";
import styles from "./OperationDetailPage.module.css";

export const OperationDetailPage = () => {
  const params = useParams();
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const isNew = () => params.id === "new";

  const [formData, setFormData] = createSignal<Partial<CustomOperation>>({
    resourceType: "CustomOperation",
    app: {
      reference: searchParams.appId ? `App/${searchParams.appId}` : "",
    },
    path: "",
    method: "GET",
    type: "proxy",
    active: true,
    description: "",
    config: { url: "", timeout: 30000, headers: {}, forwardAuth: false } as ProxyConfig,
  });

  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    await loadApps();

    if (!isNew()) {
      try {
        await loadOperation(params.id);
        const operation = selectedOperation();
        if (operation) {
          setFormData(operation);
        }
      } catch (err) {
        console.error("Failed to load operation:", err);
      }
    }
  });

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setSaving(true);
    setError(null);

    try {
      const data = formData();
      if (isNew()) {
        await createOperation(data as any);
      } else {
        await updateOperation(params.id, data as CustomOperation);
      }
      navigate("/gateway");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save operation");
    } finally {
      setSaving(false);
    }
  };

  const handleCancel = () => {
    navigate("/gateway");
  };

  const updateField = <K extends keyof CustomOperation>(
    field: K,
    value: CustomOperation[K],
  ) => {
    setFormData((prev) => ({ ...prev, [field]: value }));
  };

  const handleTypeChange = (type: OperationType) => {
    updateField("type", type);
    // Initialize default config for each type
    switch (type) {
      case "proxy":
        updateField("config", {
          url: "",
          timeout: 30000,
          headers: {},
          forwardAuth: false,
        } as ProxyConfig);
        break;
      case "sql":
        updateField("config", { query: "" } as SqlConfig);
        break;
      case "fhirpath":
        updateField("config", { expression: "" } as FhirPathConfig);
        break;
      case "handler":
        updateField("config", { name: "" } as HandlerConfig);
        break;
    }
  };

  const updateProxyConfig = <K extends keyof ProxyConfig>(
    field: K,
    value: ProxyConfig[K],
  ) => {
    setFormData((prev) => ({
      ...prev,
      config: { ...prev.config, [field]: value } as ProxyConfig,
    }));
  };

  const updateSqlConfig = (query: string) => {
    setFormData((prev) => ({
      ...prev,
      config: { query } as SqlConfig,
    }));
  };

  const updateFhirPathConfig = (expression: string) => {
    setFormData((prev) => ({
      ...prev,
      config: { expression } as FhirPathConfig,
    }));
  };

  const updateHandlerConfig = (name: string) => {
    setFormData((prev) => ({
      ...prev,
      config: { name } as HandlerConfig,
    }));
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <div>
          <h1 class={styles.title}>
            {isNew() ? "Create Custom Operation" : "Edit Custom Operation"}
          </h1>
          <p class={styles.subtitle}>
            {isNew()
              ? "Configure a new API gateway operation"
              : "Update operation configuration"}
          </p>
        </div>
      </div>

      <Show when={operationsLoading()}>
        <div class={styles.loaderContainer}>
          <Loader />
        </div>
      </Show>

      <Show when={!operationsLoading()}>
        <form onSubmit={handleSubmit}>
          <Card class={styles.formCard}>
            <h2 class={styles.sectionTitle}>Basic Information</h2>

            <div class={styles.formGroup}>
              <label class={styles.label}>App *</label>
              <Select
                required
                value={formData().app?.reference || ""}
                onChange={(e) =>
                  updateField("app", { reference: e.currentTarget.value })
                }
              >
                <option value="">Select an app</option>
                <For each={apps()}>
                  {(app) => (
                    <option value={`App/${app.id}`}>
                      {app.name} ({app.basePath})
                    </option>
                  )}
                </For>
              </Select>
            </div>

            <div class={styles.formRow}>
              <div class={styles.formGroup}>
                <label class={styles.label}>HTTP Method *</label>
                <Select
                  required
                  value={formData().method}
                  onChange={(e) =>
                    updateField("method", e.currentTarget.value as HttpMethod)
                  }
                >
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                  <option value="PUT">PUT</option>
                  <option value="DELETE">DELETE</option>
                  <option value="PATCH">PATCH</option>
                </Select>
              </div>

              <div class={styles.formGroup}>
                <label class={styles.label}>Path *</label>
                <Input
                  type="text"
                  required
                  value={formData().path}
                  onInput={(e) => updateField("path", e.currentTarget.value)}
                  placeholder="/users/:id"
                />
              </div>
            </div>

            <div class={styles.formGroup}>
              <label class={styles.label}>Description</label>
              <Input
                type="text"
                value={formData().description || ""}
                onInput={(e) => updateField("description", e.currentTarget.value)}
                placeholder="Optional description"
              />
            </div>

            <div class={styles.formGroup}>
              <label class={styles.checkboxLabel}>
                <input
                  type="checkbox"
                  checked={formData().active}
                  onChange={(e) => updateField("active", e.currentTarget.checked)}
                />
                Active
              </label>
            </div>
          </Card>

          <Card class={styles.formCard}>
            <h2 class={styles.sectionTitle}>Operation Configuration</h2>

            <div class={styles.formGroup}>
              <label class={styles.label}>Operation Type *</label>
              <Select
                required
                value={formData().type}
                onChange={(e) =>
                  handleTypeChange(e.currentTarget.value as OperationType)
                }
              >
                <option value="proxy">Proxy - Forward to external service</option>
                <option value="sql">SQL - Execute database query</option>
                <option value="fhirpath">FHIRPath - Evaluate expression</option>
                <option value="handler">Handler - Custom Rust handler</option>
              </Select>
            </div>

            <Show when={formData().type === "proxy"}>
              <div class={styles.typeConfig}>
                <div class={styles.formGroup}>
                  <label class={styles.label}>Target URL *</label>
                  <Input
                    type="url"
                    required
                    value={(formData().config as ProxyConfig)?.url || ""}
                    onInput={(e) => updateProxyConfig("url", e.currentTarget.value)}
                    placeholder="https://api.example.com/resource"
                  />
                </div>

                <div class={styles.formGroup}>
                  <label class={styles.label}>Timeout (ms)</label>
                  <Input
                    type="number"
                    value={(formData().config as ProxyConfig)?.timeout || 30000}
                    onInput={(e) =>
                      updateProxyConfig("timeout", parseInt(e.currentTarget.value))
                    }
                  />
                </div>

                <div class={styles.formGroup}>
                  <label class={styles.checkboxLabel}>
                    <input
                      type="checkbox"
                      checked={(formData().config as ProxyConfig)?.forwardAuth || false}
                      onChange={(e) =>
                        updateProxyConfig("forwardAuth", e.currentTarget.checked)
                      }
                    />
                    Forward Authentication Headers
                  </label>
                </div>
              </div>
            </Show>

            <Show when={formData().type === "sql"}>
              <div class={styles.typeConfig}>
                <div class={styles.formGroup}>
                  <label class={styles.label}>SQL Query *</label>
                  <textarea
                    class={styles.textarea}
                    required
                    value={(formData().config as SqlConfig)?.query || ""}
                    onInput={(e) => updateSqlConfig(e.currentTarget.value)}
                    placeholder="SELECT * FROM users WHERE id = $1"
                    rows={8}
                  />
                  <p class={styles.hint}>
                    Only SELECT queries are allowed. Use $1, $2, etc. for parameters.
                  </p>
                </div>
              </div>
            </Show>

            <Show when={formData().type === "fhirpath"}>
              <div class={styles.typeConfig}>
                <div class={styles.formGroup}>
                  <label class={styles.label}>FHIRPath Expression *</label>
                  <textarea
                    class={styles.textarea}
                    required
                    value={(formData().config as FhirPathConfig)?.expression || ""}
                    onInput={(e) => updateFhirPathConfig(e.currentTarget.value)}
                    placeholder="Patient.name.where(use='official').first()"
                    rows={6}
                  />
                  <p class={styles.hint}>
                    Expression will be evaluated against the request body.
                  </p>
                </div>
              </div>
            </Show>

            <Show when={formData().type === "handler"}>
              <div class={styles.typeConfig}>
                <div class={styles.formGroup}>
                  <label class={styles.label}>Handler Name *</label>
                  <Input
                    type="text"
                    required
                    value={(formData().config as HandlerConfig)?.name || ""}
                    onInput={(e) => updateHandlerConfig(e.currentTarget.value)}
                    placeholder="my_custom_handler"
                  />
                  <p class={styles.hint}>
                    Must match a handler registered in the HandlerRegistry.
                  </p>
                </div>
              </div>
            </Show>
          </Card>

          <Show when={error()}>
            <Card class={styles.errorCard}>
              <p class={styles.errorText}>{error()}</p>
            </Card>
          </Show>

          <div class={styles.actions}>
            <Button variant="outline" onClick={handleCancel} type="button">
              Cancel
            </Button>
            <Button variant="primary" type="submit" loading={saving()}>
              {isNew() ? "Create Operation" : "Save Changes"}
            </Button>
          </div>
        </form>
      </Show>
    </div>
  );
};
