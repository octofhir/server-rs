import { createSignal } from "solid-js";
import { fhirClient } from "@/shared/api";
import type {
  App,
  CustomOperation,
  NewApp,
  NewCustomOperation,
  Bundle,
} from "./types";

// Apps state
const [apps, setApps] = createSignal<App[]>([]);
const [selectedApp, setSelectedApp] = createSignal<App | null>(null);
const [appsLoading, setAppsLoading] = createSignal(false);
const [appsError, setAppsError] = createSignal<string | null>(null);

// CustomOperations state
const [operations, setOperations] = createSignal<CustomOperation[]>([]);
const [selectedOperation, setSelectedOperation] = createSignal<CustomOperation | null>(null);
const [operationsLoading, setOperationsLoading] = createSignal(false);
const [operationsError, setOperationsError] = createSignal<string | null>(null);

// App Actions
export const loadApps = async (params: Record<string, string> = {}) => {
  setAppsLoading(true);
  setAppsError(null);
  try {
    const result = await fhirClient.search("App", params);
    const appsList = (result as Bundle<App>).entry?.map((e) => e.resource).filter(Boolean) || [];
    setApps(appsList);
    return appsList;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load apps";
    setAppsError(message);
    throw err;
  } finally {
    setAppsLoading(false);
  }
};

export const loadApp = async (id: string) => {
  setAppsLoading(true);
  setAppsError(null);
  try {
    const result = await fhirClient.read("App", id);
    setSelectedApp(result as App);
    return result as App;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load app";
    setAppsError(message);
    throw err;
  } finally {
    setAppsLoading(false);
  }
};

export const createApp = async (app: NewApp) => {
  setAppsLoading(true);
  setAppsError(null);
  try {
    const result = await fhirClient.create("App", app);
    // Reload apps list
    await loadApps();
    return result as App;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to create app";
    setAppsError(message);
    throw err;
  } finally {
    setAppsLoading(false);
  }
};

export const updateApp = async (id: string, app: App) => {
  setAppsLoading(true);
  setAppsError(null);
  try {
    const result = await fhirClient.update("App", id, app);
    // Update selected app if it's the one being edited
    if (selectedApp()?.id === id) {
      setSelectedApp(result as App);
    }
    // Reload apps list
    await loadApps();
    return result as App;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to update app";
    setAppsError(message);
    throw err;
  } finally {
    setAppsLoading(false);
  }
};

export const deleteApp = async (id: string) => {
  setAppsLoading(true);
  setAppsError(null);
  try {
    await fhirClient.delete("App", id);
    // Clear selected app if it was deleted
    if (selectedApp()?.id === id) {
      setSelectedApp(null);
    }
    // Reload apps list
    await loadApps();
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to delete app";
    setAppsError(message);
    throw err;
  } finally {
    setAppsLoading(false);
  }
};

// CustomOperation Actions
export const loadOperations = async (params: Record<string, string> = {}) => {
  setOperationsLoading(true);
  setOperationsError(null);
  try {
    const result = await fhirClient.search("CustomOperation", params);
    const opsList =
      (result as Bundle<CustomOperation>).entry?.map((e) => e.resource).filter(Boolean) || [];
    setOperations(opsList);
    return opsList;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load operations";
    setOperationsError(message);
    throw err;
  } finally {
    setOperationsLoading(false);
  }
};

export const loadOperationsByApp = async (appId: string) => {
  return loadOperations({ app: `App/${appId}` });
};

export const loadOperation = async (id: string) => {
  setOperationsLoading(true);
  setOperationsError(null);
  try {
    const result = await fhirClient.read("CustomOperation", id);
    setSelectedOperation(result as CustomOperation);
    return result as CustomOperation;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load operation";
    setOperationsError(message);
    throw err;
  } finally {
    setOperationsLoading(false);
  }
};

export const createOperation = async (operation: NewCustomOperation) => {
  setOperationsLoading(true);
  setOperationsError(null);
  try {
    const result = await fhirClient.create("CustomOperation", operation);
    // Reload operations list
    await loadOperations();
    return result as CustomOperation;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to create operation";
    setOperationsError(message);
    throw err;
  } finally {
    setOperationsLoading(false);
  }
};

export const updateOperation = async (id: string, operation: CustomOperation) => {
  setOperationsLoading(true);
  setOperationsError(null);
  try {
    const result = await fhirClient.update("CustomOperation", id, operation);
    // Update selected operation if it's the one being edited
    if (selectedOperation()?.id === id) {
      setSelectedOperation(result as CustomOperation);
    }
    // Reload operations list
    await loadOperations();
    return result as CustomOperation;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to update operation";
    setOperationsError(message);
    throw err;
  } finally {
    setOperationsLoading(false);
  }
};

export const deleteOperation = async (id: string) => {
  setOperationsLoading(true);
  setOperationsError(null);
  try {
    await fhirClient.delete("CustomOperation", id);
    // Clear selected operation if it was deleted
    if (selectedOperation()?.id === id) {
      setSelectedOperation(null);
    }
    // Reload operations list
    await loadOperations();
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to delete operation";
    setOperationsError(message);
    throw err;
  } finally {
    setOperationsLoading(false);
  }
};

// Clear actions
export const clearSelectedApp = () => {
  setSelectedApp(null);
};

export const clearSelectedOperation = () => {
  setSelectedOperation(null);
};

// Exports
export {
  apps,
  selectedApp,
  setSelectedApp,
  appsLoading,
  appsError,
  operations,
  selectedOperation,
  setSelectedOperation,
  operationsLoading,
  operationsError,
};
