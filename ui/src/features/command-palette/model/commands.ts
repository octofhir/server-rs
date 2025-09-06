import type { Action } from "kbar";
import type React from "react";

export interface Command {
  id: string;
  name: string;
  shortcut?: string[];
  keywords?: string;
  section?: string;
  priority?: number;
  perform?: () => void | Promise<void>;
  subtitle?: string;
  icon?: React.ReactNode;
}

// Predefined command sections
export const COMMAND_SECTIONS = {
  NAVIGATION: "Navigation",
  ACTIONS: "Actions",
  SYSTEM: "System",
  REST_CONSOLE: "REST Console",
  THEME: "Theme",
} as const;

// System commands
export const systemCommands: Command[] = [
  {
    id: "copy-commit-sha",
    name: "Copy Commit SHA",
    shortcut: [],
    keywords: "copy commit sha git",
    section: COMMAND_SECTIONS.SYSTEM,
    priority: 100,
    perform: async () => {
      try {
        // This will be implemented when we create the API utilities
        const buildInfo = await fetch("/api/build-info").then((r) => r.json());
        await navigator.clipboard.writeText(buildInfo.commit);
      } catch (error) {
        console.error("Failed to copy commit SHA:", error);
      }
    },
  },
  {
    id: "refresh-health",
    name: "Refresh Health Status",
    shortcut: ["r", "h"],
    keywords: "refresh health status check",
    section: COMMAND_SECTIONS.SYSTEM,
    priority: 90,
    perform: () => {
      // This will be implemented when we create the health badge
      window.dispatchEvent(new CustomEvent("refresh-health"));
    },
  },
];

// Navigation commands factory function
export const createNavigationCommands = (navigate: (path: string) => void): Command[] => [
  {
    id: "open-resource-browser",
    name: "Open Resource Browser",
    shortcut: ["r", "b"],
    keywords: "resources browser fhir explore",
    section: COMMAND_SECTIONS.NAVIGATION,
    priority: 100,
    perform: () => {
      navigate("/");
    },
  },
  {
    id: "open-rest-console",
    name: "Open REST Console",
    shortcut: ["r", "c"],
    keywords: "rest console api request",
    section: COMMAND_SECTIONS.NAVIGATION,
    priority: 95,
    perform: () => {
      navigate("/console");
    },
  },
  {
    id: "open-settings",
    name: "Open Settings",
    shortcut: ["s"],
    keywords: "settings preferences config",
    section: COMMAND_SECTIONS.NAVIGATION,
    priority: 80,
    perform: () => {
      navigate("/settings");
    },
  },
];

// Default navigation commands (fallback)
export const navigationCommands: Command[] = createNavigationCommands(() => {
  console.warn("Navigation function not provided, commands may not work correctly");
});

// Action commands
export const actionCommands: Command[] = [
  {
    id: "send-rest-request",
    name: "Send REST Request",
    shortcut: ["ctrl+enter", "cmd+enter"],
    keywords: "send request execute run",
    section: COMMAND_SECTIONS.ACTIONS,
    priority: 100,
    perform: () => {
      // Dispatch event for REST console to handle
      window.dispatchEvent(new CustomEvent("send-request"));
    },
  },
  {
    id: "toggle-theme",
    name: "Toggle Theme",
    shortcut: ["ctrl+shift+t", "cmd+shift+t"],
    keywords: "theme dark light toggle",
    section: COMMAND_SECTIONS.THEME,
    priority: 85,
    perform: () => {
      // This will be implemented when we have theme management
      window.dispatchEvent(new CustomEvent("toggle-theme"));
    },
  },
  {
    id: "rebuild-theme",
    name: "Rebuild Theme from Logo",
    shortcut: [],
    keywords: "theme rebuild logo colors",
    section: COMMAND_SECTIONS.THEME,
    priority: 70,
    perform: () => {
      window.dispatchEvent(new CustomEvent("rebuild-theme"));
    },
  },
];

// REST Console specific commands
export const restConsoleCommands: Command[] = [
  {
    id: "clear-response",
    name: "Clear Response",
    shortcut: ["ctrl+k", "cmd+k"],
    keywords: "clear response reset",
    section: COMMAND_SECTIONS.REST_CONSOLE,
    priority: 90,
    perform: () => {
      window.dispatchEvent(new CustomEvent("clear-response"));
    },
  },
  {
    id: "save-preset",
    name: "Save as Preset",
    shortcut: ["ctrl+s", "cmd+s"],
    keywords: "save preset bookmark",
    section: COMMAND_SECTIONS.REST_CONSOLE,
    priority: 80,
    perform: () => {
      window.dispatchEvent(new CustomEvent("save-preset"));
    },
  },
  {
    id: "load-preset",
    name: "Load Preset",
    shortcut: ["ctrl+o", "cmd+o"],
    keywords: "load preset open",
    section: COMMAND_SECTIONS.REST_CONSOLE,
    priority: 75,
    perform: () => {
      window.dispatchEvent(new CustomEvent("load-preset"));
    },
  },
];

// Combine all commands
export const defaultCommands: Command[] = [
  ...systemCommands,
  ...navigationCommands,
  ...actionCommands,
  ...restConsoleCommands,
];

// Helper to convert our commands to kbar actions
export const commandToAction = (command: Command): Action => ({
  ...command,
  perform: command.perform || (() => {}),
});
