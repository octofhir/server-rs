import { createSignal, onMount, For, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import {
  Button,
  Card,
  Loader,
  StatusBadge,
  Table,
  TableHead,
  TableBody,
  TableRow,
  TableCell,
  TableHeaderCell,
  Input,
  Modal,
} from "@/shared/ui";
import {
  loadApps,
  deleteApp,
  apps,
  appsLoading,
  appsError,
  type App,
} from "@/entities/gateway";
import styles from "./GatewayPage.module.css";

export const GatewayPage = () => {
  const navigate = useNavigate();
  const [searchTerm, setSearchTerm] = createSignal("");
  const [showDeleteModal, setShowDeleteModal] = createSignal(false);
  const [appToDelete, setAppToDelete] = createSignal<App | null>(null);

  onMount(async () => {
    try {
      await loadApps();
    } catch (err) {
      console.error("Failed to load apps:", err);
    }
  });

  const filteredApps = () => {
    const term = searchTerm().toLowerCase();
    if (!term) return apps();
    return apps().filter(
      (app) =>
        app.name.toLowerCase().includes(term) ||
        app.description?.toLowerCase().includes(term) ||
        app.basePath.toLowerCase().includes(term),
    );
  };

  const handleRowClick = (app: App) => {
    if (app.id) {
      navigate(`/gateway/apps/${app.id}`);
    }
  };

  const handleCreateApp = () => {
    navigate("/gateway/apps/new");
  };

  const handleDeleteClick = (e: MouseEvent, app: App) => {
    e.stopPropagation();
    setAppToDelete(app);
    setShowDeleteModal(true);
  };

  const handleConfirmDelete = async () => {
    const app = appToDelete();
    if (app?.id) {
      try {
        await deleteApp(app.id);
        setShowDeleteModal(false);
        setAppToDelete(null);
      } catch (err) {
        console.error("Failed to delete app:", err);
      }
    }
  };

  const handleCancelDelete = () => {
    setShowDeleteModal(false);
    setAppToDelete(null);
  };

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <div>
          <h1 class={styles.title}>API Gateway</h1>
          <p class={styles.subtitle}>
            Manage custom API endpoints and proxy configurations
          </p>
        </div>
        <Button variant="primary" onClick={handleCreateApp}>
          Create App
        </Button>
      </div>

      <Card class={styles.searchCard}>
        <Input
          type="text"
          placeholder="Search apps by name, description, or base path..."
          value={searchTerm()}
          onInput={(e) => setSearchTerm(e.currentTarget.value)}
          class={styles.searchInput}
        />
      </Card>

      <Show when={appsLoading()}>
        <div class={styles.loaderContainer}>
          <Loader />
        </div>
      </Show>

      <Show when={appsError()}>
        <Card class={styles.errorCard}>
          <p class={styles.errorText}>{appsError()}</p>
        </Card>
      </Show>

      <Show when={!appsLoading() && !appsError()}>
        <Show
          when={filteredApps().length > 0}
          fallback={
            <Card class={styles.emptyCard}>
              <p class={styles.emptyText}>
                {searchTerm()
                  ? "No apps match your search criteria."
                  : "No apps configured. Create your first app to get started."}
              </p>
            </Card>
          }
        >
          <Table hoverable>
            <TableHead>
              <TableRow>
                <TableHeaderCell>Name</TableHeaderCell>
                <TableHeaderCell>Base Path</TableHeaderCell>
                <TableHeaderCell>Description</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>Auth</TableHeaderCell>
                <TableHeaderCell>Actions</TableHeaderCell>
              </TableRow>
            </TableHead>
            <TableBody>
              <For each={filteredApps()}>
                {(app) => (
                  <TableRow onClick={() => handleRowClick(app)}>
                    <TableCell>
                      <span class={styles.appName}>{app.name}</span>
                    </TableCell>
                    <TableCell>
                      <code class={styles.basePath}>{app.basePath}</code>
                    </TableCell>
                    <TableCell>
                      <span class={styles.description}>
                        {app.description || "-"}
                      </span>
                    </TableCell>
                    <TableCell>
                      <StatusBadge variant={app.active ? "success" : "neutral"}>
                        {app.active ? "Active" : "Inactive"}
                      </StatusBadge>
                    </TableCell>
                    <TableCell>
                      <span class={styles.auth}>
                        {app.authentication?.type || "none"}
                      </span>
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={(e: MouseEvent) => handleDeleteClick(e, app)}
                      >
                        Delete
                      </Button>
                    </TableCell>
                  </TableRow>
                )}
              </For>
            </TableBody>
          </Table>
        </Show>
      </Show>

      <Modal
        open={showDeleteModal()}
        onClose={handleCancelDelete}
        title="Delete App"
        size="sm"
      >
        <div class={styles.modalContent}>
          <p>
            Are you sure you want to delete the app <strong>{appToDelete()?.name}</strong>?
            This will also delete all associated custom operations.
          </p>
          <div class={styles.modalActions}>
            <Button variant="outline" onClick={handleCancelDelete}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleConfirmDelete}>
              Delete
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};
