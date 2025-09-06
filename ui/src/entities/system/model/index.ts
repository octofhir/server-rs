import { createEffect, createEvent, createStore, sample } from "effector";
import type { BuildInfo, HealthResponse } from "@/shared/api";
import { serverApi } from "@/shared/api";

// Events
export const getHealthFx = createEffect<void, HealthResponse>();
export const getBuildInfoFx = createEffect<void, BuildInfo>();
export const getResourceTypesFx = createEffect<void, string[]>();

export const resetSystemState = createEvent();
export const setConnectionStatus = createEvent<"connected" | "disconnected" | "connecting">();

// Stores
export const $health = createStore<HealthResponse | null>(null);
export const $buildInfo = createStore<BuildInfo | null>(null);
export const $resourceTypes = createStore<string[]>([]);
export const $connectionStatus = createStore<"connected" | "disconnected" | "connecting">(
  "disconnected"
);
export const $systemLoading = createStore(false);
export const $systemError = createStore<string | null>(null);

// Effects implementations
getHealthFx.use(async () => {
  return await serverApi.getHealth();
});

getBuildInfoFx.use(async () => {
  return await serverApi.getBuildInfo();
});

getResourceTypesFx.use(async () => {
  return await serverApi.getResourceTypes();
});

// Store updates
$health.on(getHealthFx.doneData, (_, health) => health);
$buildInfo.on(getBuildInfoFx.doneData, (_, buildInfo) => buildInfo);
$resourceTypes.on(getResourceTypesFx.doneData, (_, types) => types);
$connectionStatus.on(setConnectionStatus, (_, status) => status);

// Loading states
$systemLoading
  .on(getHealthFx.pending, (_, pending) => pending)
  .on(getBuildInfoFx.pending, (_, pending) => pending)
  .on(getResourceTypesFx.pending, (_, pending) => pending);

// Error handling
$systemError
  .on(getHealthFx.failData, (_, error) => error?.message || "Health check failed")
  .on(getBuildInfoFx.failData, (_, error) => error?.message || "Build info fetch failed")
  .on(getResourceTypesFx.failData, (_, error) => error?.message || "Resource types fetch failed")
  .reset(getHealthFx, getBuildInfoFx, getResourceTypesFx);

// Reset state
sample({
  clock: resetSystemState,
  target: $health.reinit!,
});

sample({
  clock: resetSystemState,
  target: $buildInfo.reinit!,
});

sample({
  clock: resetSystemState,
  target: $resourceTypes.reinit!,
});

sample({
  clock: resetSystemState,
  target: $systemError.reinit!,
});

// Auto-update connection status based on health checks
sample({
  clock: getHealthFx,
  fn: () => "connecting" as const,
  target: setConnectionStatus,
});

sample({
  clock: getHealthFx.done,
  fn: () => "connected" as const,
  target: setConnectionStatus,
});

sample({
  clock: getHealthFx.fail,
  fn: () => "disconnected" as const,
  target: setConnectionStatus,
});
