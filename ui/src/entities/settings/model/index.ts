import { createEvent, createStore } from "effector";
import { storage } from "@/shared/lib/storage";

// Theme settings
export const setColorScheme = createEvent<"light" | "dark" | "auto">();
export const setThemeInitialized = createEvent<boolean>();

// API settings
export const setApiBaseUrl = createEvent<string>();
export const setApiTimeout = createEvent<number>();
export const toggleAutoRefresh = createEvent();
export const setAutoRefreshInterval = createEvent<number>();

// UI preferences
export const toggleCompactMode = createEvent();
export const setJsonViewerTheme = createEvent<"light" | "dark">();
export const toggleLineNumbers = createEvent();
export const setSyntaxHighlighting = createEvent<boolean>();

// Reset
export const resetSettingsState = createEvent();

// Persistence keys
const COLOR_SCHEME_KEY = "settings:colorScheme";

// Stores
export const $colorScheme = createStore<"light" | "dark" | "auto">(
  storage.getItem<"light" | "dark" | "auto">(COLOR_SCHEME_KEY, "auto") ?? "auto"
);
export const $themeInitialized = createStore(false);

export const $apiBaseUrl = createStore("http://localhost:8080");
export const $apiTimeout = createStore(5000);
export const $autoRefresh = createStore(false);
export const $autoRefreshInterval = createStore(30000); // 30 seconds

export const $compactMode = createStore(false);
export const $jsonViewerTheme = createStore<"light" | "dark">("light");
export const $lineNumbers = createStore(true);
export const $syntaxHighlighting = createStore(true);

// Store updates
$colorScheme.on(setColorScheme, (_, scheme) => scheme);
$themeInitialized.on(setThemeInitialized, (_, initialized) => initialized);

$apiBaseUrl.on(setApiBaseUrl, (_, url) => url);
$apiTimeout.on(setApiTimeout, (_, timeout) => timeout);
$autoRefresh.on(toggleAutoRefresh, (enabled) => !enabled);
$autoRefreshInterval.on(setAutoRefreshInterval, (_, interval) => interval);

$compactMode.on(toggleCompactMode, (compact) => !compact);
$jsonViewerTheme.on(setJsonViewerTheme, (_, theme) => theme);
$lineNumbers.on(toggleLineNumbers, (enabled) => !enabled);
$syntaxHighlighting.on(setSyntaxHighlighting, (_, enabled) => enabled);

// Persist on change
setColorScheme.watch((scheme) => {
  try {
    storage.setItem(COLOR_SCHEME_KEY, scheme);
  } catch (e) {
    // non-fatal
    console.warn("Failed to persist color scheme", e);
  }
});

// Reset all settings
$colorScheme.reset(resetSettingsState);
$themeInitialized.reset(resetSettingsState);
$apiBaseUrl.reset(resetSettingsState);
$apiTimeout.reset(resetSettingsState);
$autoRefresh.reset(resetSettingsState);
$autoRefreshInterval.reset(resetSettingsState);
$compactMode.reset(resetSettingsState);
$jsonViewerTheme.reset(resetSettingsState);
$lineNumbers.reset(resetSettingsState);
$syntaxHighlighting.reset(resetSettingsState);
