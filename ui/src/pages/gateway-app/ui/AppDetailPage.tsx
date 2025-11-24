import { createSignal, onMount, Show, For } from "solid-js";
import { useParams, useNavigate } from "@solidjs/router";
import {
  Button,
  Card,
  Input,
  Loader,
  Select,
  StatusBadge,
  Table,
  TableHead,
  TableBody,
  TableRow,
  TableCell,
  TableHeaderCell,
} from "@/shared/ui";
import {
  loadApp,
  createApp,
  updateApp,
  loadOperationsByApp,
  operations,
  selectedApp,
  appsLoading,
  appsError,
  operationsLoading,
  type App,
  type AuthenticationType,
} from "@/entities/gateway";
import styles from "./AppDetailPage.module.css";

export const AppDetailPage = () => {
  const params = useParams();
  const navigate = useNavigate();
  const isNew = () => params.id === "new";

  const [formData, setFormData] = createSignal<Partial<App>>({
    resourceType: "App",
    name: "",
    basePath: "",
    description: "",
    active: true,
    authentication: {
      type: "none",
      required: false,
    },
  });

  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    if (!isNew()) {
      try {
        await loadApp(params.id);
        await loadOperationsByApp(params.id);
        const app = selectedApp();
        if (app) {
          setFormData(app);
        }
      } catch (err) {
        console.error("Failed to load app:", err);
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
        await createApp(data as any);
      } else {
        await updateApp(params.id, data as App);
      }
      navigate("/gateway");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save app");
    } finally {
      setSaving(false);
    }
  };

  const handleCancel = () => {
    navigate("/gateway");
  };

  const handleOperationClick = (operationId: string) => {
    navigate(`/gateway/operations/${operationId}`);
  };

  const handleCreateOperation = () => {
    navigate(`/gateway/operations/new?appId=${params.id}`);
  };

  const updateField = <K extends keyof App>(field: K, value: App[K]) => {
    setFormData((prev) => ({ ...prev, [field]: value }));
  };

  const updateAuthField = <K extends keyof NonNullable<App["authentication"]>>(
    field: K,
    value: NonNullable<App["authentication"]>[K],
  ) => {
    setFormData((prev) => ({
      ...prev,
      authentication: { ...prev.authentication, [field]: value } as any,
    }));
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <div>
          <h1 class={styles.title}>{isNew() ? "Create App" : "Edit App"}</h1>
          <p class={styles.subtitle}>
            {isNew()
              ? "Configure a new API gateway application"
              : "Update application configuration"}
          </p>
        </div>
      </div>

      <Show when={appsLoading()}>
        <div class={styles.loaderContainer}>
          <Loader />
        </div>
      </Show>

      <Show when={!appsLoading()}>
        <form onSubmit={handleSubmit}>
          <Card class={styles.formCard}>
            <h2 class={styles.sectionTitle}>Basic Information</h2>

            <div class={styles.formGroup}>
              <label class={styles.label}>Name *</label>
              <Input
                type="text"
                required
                value={formData().name}
                onInput={(e) => updateField("name", e.currentTarget.value)}
                placeholder="My API"
              />
            </div>

            <div class={styles.formGroup}>
              <label class={styles.label}>Base Path *</label>
              <Input
                type="text"
                required
                value={formData().basePath}
                onInput={(e) => updateField("basePath", e.currentTarget.value)}
                placeholder="/api/v1"
              />
              <p class={styles.hint}>
                All operations will be mounted under this base path
              </p>
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
              <p class={styles.hint}>
                Only active apps will have their routes registered
              </p>
            </div>
          </Card>

          <Card class={styles.formCard}>
            <h2 class={styles.sectionTitle}>Authentication</h2>

            <div class={styles.formGroup}>
              <label class={styles.label}>Authentication Type</label>
              <Select
                value={formData().authentication?.type || "none"}
                onChange={(e) =>
                  updateAuthField("type", e.currentTarget.value as AuthenticationType)
                }
              >
                <option value="none">None</option>
                <option value="bearer">Bearer Token</option>
                <option value="basic">Basic Auth</option>
                <option value="api-key">API Key</option>
              </Select>
            </div>

            <div class={styles.formGroup}>
              <label class={styles.checkboxLabel}>
                <input
                  type="checkbox"
                  checked={formData().authentication?.required || false}
                  onChange={(e) =>
                    updateAuthField("required", e.currentTarget.checked)
                  }
                />
                Authentication Required
              </label>
            </div>
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
              {isNew() ? "Create App" : "Save Changes"}
            </Button>
          </div>
        </form>

        <Show when={!isNew() && !appsLoading()}>
          <Card class={styles.operationsCard}>
            <div class={styles.operationsHeader}>
              <h2 class={styles.sectionTitle}>Custom Operations</h2>
              <Button variant="primary" size="sm" onClick={handleCreateOperation}>
                Add Operation
              </Button>
            </div>

            <Show when={operationsLoading()}>
              <div class={styles.loaderContainer}>
                <Loader />
              </div>
            </Show>

            <Show when={!operationsLoading()}>
              <Show
                when={operations().length > 0}
                fallback={
                  <p class={styles.emptyText}>
                    No operations configured. Add your first operation to get started.
                  </p>
                }
              >
                <Table hoverable>
                  <TableHead>
                    <TableRow>
                      <TableHeaderCell>Method</TableHeaderCell>
                      <TableHeaderCell>Path</TableHeaderCell>
                      <TableHeaderCell>Type</TableHeaderCell>
                      <TableHeaderCell>Status</TableHeaderCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    <For each={operations()}>
                      {(operation) => (
                        <TableRow onClick={() => handleOperationClick(operation.id!)}>
                          <TableCell>
                            <code class={styles.method}>{operation.method}</code>
                          </TableCell>
                          <TableCell>
                            <code class={styles.path}>{operation.path}</code>
                          </TableCell>
                          <TableCell>
                            <span class={styles.type}>{operation.type}</span>
                          </TableCell>
                          <TableCell>
                            <StatusBadge
                              variant={operation.active ? "success" : "neutral"}
                            >
                              {operation.active ? "Active" : "Inactive"}
                            </StatusBadge>
                          </TableCell>
                        </TableRow>
                      )}
                    </For>
                  </TableBody>
                </Table>
              </Show>
            </Show>
          </Card>
        </Show>
      </Show>
    </div>
  );
};
