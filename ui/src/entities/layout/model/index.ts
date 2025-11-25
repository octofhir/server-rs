import { createSignal, createEffect, type Accessor } from "solid-js";
import { createEvent, createStore } from "effector";

// Events
export const toggleSidebar = createEvent();
export const setSidebarWidth = createEvent<number>();
export const setSidebarCollapsed = createEvent<boolean>();
export const resetLayoutState = createEvent();

// Stores
export const $sidebarOpened = createStore(true);
export const $sidebarWidth = createStore(280);
export const $sidebarCollapsed = createStore(false);

// Store updates
$sidebarOpened.on(toggleSidebar, (opened) => !opened);
$sidebarWidth.on(setSidebarWidth, (_, width) => width);
$sidebarCollapsed.on(setSidebarCollapsed, (_, collapsed) => collapsed);

// Reset
$sidebarOpened.reset(resetLayoutState);
$sidebarWidth.reset(resetLayoutState);
$sidebarCollapsed.reset(resetLayoutState);

// Local storage helper for SolidJS
function createLocalStorageSignal<T>(
  key: string,
  defaultValue: T
): [Accessor<T>, (value: T) => void] {
  // Get initial value from localStorage or use default
  const getStoredValue = (): T => {
    if (typeof window === "undefined") return defaultValue;
    try {
      const item = window.localStorage.getItem(key);
      return item ? JSON.parse(item) : defaultValue;
    } catch {
      return defaultValue;
    }
  };

  const [value, setValue] = createSignal<T>(getStoredValue());

  // Sync to localStorage when value changes
  createEffect(() => {
    const currentValue = value();
    if (typeof window !== "undefined") {
      try {
        window.localStorage.setItem(key, JSON.stringify(currentValue));
      } catch (error) {
        console.warn(`Failed to save ${key} to localStorage:`, error);
      }
    }
  });

  return [value, setValue];
}

// Persist sidebar state to localStorage
export const useSidebarPersistence = () => {
  const [sidebarWidth, setSidebarWidthLS] = createLocalStorageSignal(
    "octofhir-sidebar-width",
    280
  );

  const [sidebarOpened, setSidebarOpenedLS] = createLocalStorageSignal(
    "octofhir-sidebar-opened",
    true
  );

  return {
    sidebarWidth,
    setSidebarWidth: setSidebarWidthLS,
    sidebarOpened,
    setSidebarOpened: setSidebarOpenedLS,
  };
};
