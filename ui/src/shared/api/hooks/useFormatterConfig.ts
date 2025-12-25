/**
 * Hook for managing LSP SQL formatter configuration.
 *
 * Stores formatter settings in the _configuration table via admin API:
 * - Category: "features"
 * - Key: "lsp.formatter"
 */
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import type {
  FormatterConfig,
  FormatterStyle,
} from '../../settings/formatterTypes';
import {
  DEFAULT_FORMATTER_CONFIG,
  getDefaultConfigForStyle,
} from '../../settings/formatterTypes';

// =============================================================================
// Constants
// =============================================================================

/** Configuration category for formatter settings */
const CONFIG_CATEGORY = 'db_console';
/** Configuration key for formatter settings */
const CONFIG_KEY = 'formatter';
/** Local storage key for caching formatter config when not authenticated */
const LOCAL_STORAGE_KEY = 'octofhir:formatter-config';

// =============================================================================
// Query Keys
// =============================================================================

export const formatterKeys = {
  all: ['formatter'] as const,
  config: () => [...formatterKeys.all, 'config'] as const,
};

// =============================================================================
// API Functions
// =============================================================================

interface ConfigEntryResponse {
  key: string;
  category: string;
  value: FormatterConfig;
  description: string | null;
  is_secret: boolean;
}

/**
 * Fetch formatter config from the server.
 * Falls back to local storage if not authenticated or on error.
 */
async function fetchFormatterConfig(): Promise<FormatterConfig> {
  try {
    const response = await fetch(
      `/admin/config/${CONFIG_CATEGORY}/${CONFIG_KEY}`,
      {
        credentials: 'include',
        headers: {
          Accept: 'application/json',
        },
      },
    );

    if (response.status === 404) {
      // Config not set yet, return default
      return getLocalConfig() ?? DEFAULT_FORMATTER_CONFIG;
    }

    if (response.status === 401 || response.status === 403) {
      // Not authenticated, use local storage
      return getLocalConfig() ?? DEFAULT_FORMATTER_CONFIG;
    }

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }

    const data: ConfigEntryResponse = await response.json();
    return data.value;
  } catch (error) {
    console.warn('Failed to fetch formatter config, using local storage:', error);
    return getLocalConfig() ?? DEFAULT_FORMATTER_CONFIG;
  }
}

/**
 * Save formatter config to the server.
 * Also caches to local storage for offline access.
 */
async function saveFormatterConfig(config: FormatterConfig): Promise<FormatterConfig> {
  // Always save to local storage for quick access
  setLocalConfig(config);

  try {
    const response = await fetch(
      `/admin/config/${CONFIG_CATEGORY}/${CONFIG_KEY}`,
      {
        method: 'PUT',
        credentials: 'include',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'application/json',
        },
        body: JSON.stringify({
          value: config,
          description: 'LSP SQL formatter configuration',
          is_secret: false,
        }),
      },
    );

    if (response.status === 401 || response.status === 403) {
      // Not authenticated, local storage was already updated
      console.info('Not authenticated, formatter config saved to local storage only');
      return config;
    }

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }

    const data: ConfigEntryResponse = await response.json();
    return data.value;
  } catch (error) {
    console.warn('Failed to save formatter config to server, using local storage:', error);
    return config;
  }
}

// =============================================================================
// Local Storage Helpers
// =============================================================================

function getLocalConfig(): FormatterConfig | null {
  try {
    const stored = localStorage.getItem(LOCAL_STORAGE_KEY);
    if (stored) {
      return JSON.parse(stored) as FormatterConfig;
    }
  } catch {
    // Ignore parse errors
  }
  return null;
}

function setLocalConfig(config: FormatterConfig): void {
  try {
    localStorage.setItem(LOCAL_STORAGE_KEY, JSON.stringify(config));
  } catch {
    // Ignore storage errors (e.g., quota exceeded)
  }
}

// =============================================================================
// React Hooks
// =============================================================================

/**
 * Hook to fetch the current formatter configuration.
 *
 * @returns Query result with formatter config
 */
export function useFormatterConfig() {
  return useQuery({
    queryKey: formatterKeys.config(),
    queryFn: fetchFormatterConfig,
    staleTime: 1000 * 60 * 5, // 5 minutes
    gcTime: 1000 * 60 * 30, // 30 minutes
    // Don't refetch on window focus for settings
    refetchOnWindowFocus: false,
  });
}

/**
 * Hook to save formatter configuration.
 * Invalidates the config query on success.
 *
 * @returns Mutation for saving formatter config
 */
export function useSaveFormatterConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: saveFormatterConfig,
    onSuccess: (data) => {
      // Update the cache with the new config
      queryClient.setQueryData(formatterKeys.config(), data);
    },
  });
}

/**
 * Hook to get formatter config with a convenient save function.
 * This is the main hook for components that need both read and write access.
 *
 * @returns Object with config, loading state, and save function
 */
export function useFormatterSettings() {
  const { data: config, isLoading, error } = useFormatterConfig();
  const { mutate: saveConfig, isPending: isSaving } = useSaveFormatterConfig();

  return {
    /** Current formatter configuration (defaults to sql_style if not loaded) */
    config: config ?? DEFAULT_FORMATTER_CONFIG,
    /** Whether the config is being loaded */
    isLoading,
    /** Whether the config is being saved */
    isSaving,
    /** Any error that occurred */
    error,
    /**
     * Save a new formatter configuration.
     * @param newConfig - The new configuration to save
     */
    saveConfig: (newConfig: FormatterConfig) => saveConfig(newConfig),
    /**
     * Change the formatter style, applying default settings for that style.
     * @param style - The new style to use
     */
    setStyle: (style: FormatterStyle) => {
      const newConfig = getDefaultConfigForStyle(style);
      saveConfig(newConfig);
    },
  };
}
