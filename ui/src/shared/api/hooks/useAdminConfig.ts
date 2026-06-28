/**
 * Hooks for the runtime configuration admin API.
 *
 * Backend endpoints (require AdminAuth):
 * - GET    /admin/config/:category          → merged effective values (file + env + db)
 * - PUT    /admin/config/:category/:key      → write a DB override for a single key
 * - DELETE /admin/config/:category/:key      → reset a key back to file/env/default
 * - POST   /admin/config/$reload             → force reload from all sources
 * - GET    /admin/features                   → list feature flags
 * - PUT    /admin/features/:name             → toggle a feature flag
 *
 * Reads use the *merged category* endpoint so the UI always shows the value that
 * is actually in effect, regardless of whether it came from the TOML file, an
 * env var or a DB override. Writes go to the per-key endpoint.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isRecord } from "../guards";

// =============================================================================
// Types
// =============================================================================

/** Config categories exposed by the backend (see ConfigCategory in Rust). */
export type ConfigCategory =
  | "server"
  | "search"
  | "terminology"
  | "auth"
  | "cache"
  | "features"
  | "logging"
  | "otel"
  | "storage"
  | "redis"
  | "validation"
  | "fhir"
  | "packages"
  | "db_console";

/** Merged value of a category — a flat-ish JSON object of key → value. */
export type CategoryConfig = Record<string, unknown>;

export interface FeatureFlag {
  name: string;
  enabled: boolean;
  flag_type: string;
  description: string | null;
}

function isFeatureFlag(value: unknown): value is FeatureFlag {
  return isRecord(value) && typeof value.name === "string" && typeof value.enabled === "boolean";
}

// =============================================================================
// Query keys
// =============================================================================

export const adminConfigKeys = {
  all: ["admin-config"] as const,
  category: (category: ConfigCategory) => [...adminConfigKeys.all, "category", category] as const,
  features: () => [...adminConfigKeys.all, "features"] as const,
};

// =============================================================================
// API functions
// =============================================================================

/** Categories whose changes take effect immediately, without a restart. */
export const LIVE_CATEGORIES: ReadonlySet<ConfigCategory> = new Set([
  "logging",
  "otel",
  "search",
  "terminology",
  "packages",
  "features",
  "db_console",
]);

async function fetchCategory(category: ConfigCategory): Promise<CategoryConfig | null> {
  const response = await fetch(`/admin/config/${category}`, {
    credentials: "include",
    headers: { Accept: "application/json" },
  });

  // Not authenticated or category has no values yet → treat as empty.
  if (response.status === 401 || response.status === 403 || response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }

  const data: unknown = await response.json();
  return isRecord(data) ? (data as CategoryConfig) : {};
}

export interface SetConfigArgs {
  category: ConfigCategory;
  key: string;
  value: unknown;
  description?: string;
  isSecret?: boolean;
}

async function setConfigValue({
  category,
  key,
  value,
  description,
  isSecret = false,
}: SetConfigArgs): Promise<void> {
  const response = await fetch(`/admin/config/${category}/${key}`, {
    method: "PUT",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({
      value,
      description: description ?? null,
      is_secret: isSecret,
    }),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }
}

async function fetchFeatureFlags(): Promise<FeatureFlag[]> {
  const response = await fetch("/admin/features", {
    credentials: "include",
    headers: { Accept: "application/json" },
  });
  if (response.status === 401 || response.status === 403) {
    return [];
  }
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }
  const data: unknown = await response.json();
  return Array.isArray(data) ? data.filter(isFeatureFlag) : [];
}

async function toggleFeatureFlag({
  name,
  enabled,
}: {
  name: string;
  enabled: boolean;
}): Promise<void> {
  const response = await fetch(`/admin/features/${name}`, {
    method: "PUT",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({ enabled }),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }
}

async function reloadConfig(): Promise<void> {
  const response = await fetch("/admin/config/$reload", {
    method: "POST",
    credentials: "include",
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }
}

// =============================================================================
// React hooks
// =============================================================================

/** Read the merged effective values for a config category. */
export function useConfigCategory(category: ConfigCategory, enabled = true) {
  return useQuery({
    queryKey: adminConfigKeys.category(category),
    queryFn: () => fetchCategory(category),
    enabled,
    staleTime: 1000 * 30,
    refetchOnWindowFocus: false,
  });
}

/** Write a single config key (DB override). Invalidates the category on success. */
export function useSetConfigValue() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: setConfigValue,
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: adminConfigKeys.category(variables.category),
      });
    },
  });
}

/** List all feature flags. */
export function useFeatureFlags() {
  return useQuery({
    queryKey: adminConfigKeys.features(),
    queryFn: fetchFeatureFlags,
    staleTime: 1000 * 30,
    refetchOnWindowFocus: false,
  });
}

/** Toggle a feature flag. Invalidates the feature list on success. */
export function useToggleFeature() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: toggleFeatureFlag,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminConfigKeys.features() });
    },
  });
}

/** Force the server to reload configuration from all sources. */
export function useReloadConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: reloadConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminConfigKeys.all });
    },
  });
}
