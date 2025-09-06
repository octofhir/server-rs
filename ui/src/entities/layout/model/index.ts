import { useLocalStorage } from "@mantine/hooks";
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

// Persist sidebar state to localStorage
export const useSidebarPersistence = () => {
  const [sidebarWidth, setSidebarWidthLS] = useLocalStorage({
    key: "octofhir-sidebar-width",
    defaultValue: 280,
  });

  const [sidebarOpened, setSidebarOpenedLS] = useLocalStorage({
    key: "octofhir-sidebar-opened",
    defaultValue: true,
  });

  return {
    sidebarWidth,
    setSidebarWidth: setSidebarWidthLS,
    sidebarOpened,
    setSidebarOpened: setSidebarOpenedLS,
  };
};
