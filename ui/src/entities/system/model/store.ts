import { createSignal } from "solid-js";
import { serverApi } from "@/shared/api";
import type { BuildInfo, ServerHealth } from "@/shared/api";

export type ConnectionStatus = "connected" | "connecting" | "disconnected";

// System state
const [health, setHealth] = createSignal<ServerHealth | null>(null);
const [buildInfo, setBuildInfo] = createSignal<BuildInfo | null>(null);
const [connectionStatus, setConnectionStatus] = createSignal<ConnectionStatus>("disconnected");
const [systemLoading, setSystemLoading] = createSignal(false);
const [systemError, setSystemError] = createSignal<string | null>(null);

// Actions
export const checkHealth = async () => {
  setConnectionStatus("connecting");
  try {
    const result = await serverApi.getHealth();
    setHealth(result);
    setConnectionStatus(result.status === "healthy" ? "connected" : "disconnected");
    return result;
  } catch (err) {
    setConnectionStatus("disconnected");
    const message = err instanceof Error ? err.message : "Health check failed";
    setSystemError(message);
    throw err;
  }
};

export const loadBuildInfo = async () => {
  setSystemLoading(true);
  try {
    const result = await serverApi.getBuildInfo();
    setBuildInfo(result);
    return result;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load build info";
    setSystemError(message);
    throw err;
  } finally {
    setSystemLoading(false);
  }
};

// Start health polling
let healthInterval: ReturnType<typeof setInterval> | null = null;

export const startHealthPolling = (intervalMs = 15000) => {
  stopHealthPolling();
  checkHealth();
  healthInterval = setInterval(checkHealth, intervalMs);
};

export const stopHealthPolling = () => {
  if (healthInterval) {
    clearInterval(healthInterval);
    healthInterval = null;
  }
};

// Exports
export {
  health,
  buildInfo,
  connectionStatus,
  setConnectionStatus,
  systemLoading,
  systemError,
};
