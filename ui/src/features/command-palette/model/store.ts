import { createEvent, createStore } from "effector";
import { type Command, defaultCommands } from "./commands";

// Events
export const addCommand = createEvent<Command>("add command");
export const removeCommand = createEvent<string>("remove command");
export const updateCommand = createEvent<{ id: string; updates: Partial<Command> }>(
  "update command"
);
export const setCommands = createEvent<Command[]>("set commands");
export const resetCommands = createEvent("reset commands");

// Stores
export const $commands = createStore<Command[]>(defaultCommands);
export const $commandsById = $commands.map((commands) =>
  commands.reduce(
    (acc, command) => {
      acc[command.id] = command;
      return acc;
    },
    {} as Record<string, Command>
  )
);

// Command registration state
export const $isCommandPaletteOpen = createStore(false);
export const toggleCommandPalette = createEvent("toggle command palette");
export const openCommandPalette = createEvent("open command palette");
export const closeCommandPalette = createEvent("close command palette");

// Update stores based on events
$commands
  .on(addCommand, (commands, newCommand) => {
    const existingIndex = commands.findIndex((cmd) => cmd.id === newCommand.id);
    if (existingIndex >= 0) {
      // Replace existing command
      const updated = [...commands];
      updated[existingIndex] = newCommand;
      return updated;
    }
    return [...commands, newCommand];
  })
  .on(removeCommand, (commands, commandId) => commands.filter((cmd) => cmd.id !== commandId))
  .on(updateCommand, (commands, { id, updates }) =>
    commands.map((cmd) => (cmd.id === id ? { ...cmd, ...updates } : cmd))
  )
  .on(setCommands, (_, commands) => commands)
  .on(resetCommands, () => defaultCommands);

$isCommandPaletteOpen
  .on(toggleCommandPalette, (isOpen) => !isOpen)
  .on(openCommandPalette, () => true)
  .on(closeCommandPalette, () => false);

// Keyboard shortcut handling
const handleGlobalKeydown = (event: KeyboardEvent) => {
  const { key, ctrlKey, metaKey } = event;
  const isModifierPressed = ctrlKey || metaKey;

  // Ctrl/Cmd + K opens command palette
  if (isModifierPressed && key.toLowerCase() === "k") {
    event.preventDefault();
    toggleCommandPalette();
    return;
  }

  // Escape closes command palette
  if (key === "Escape") {
    closeCommandPalette();
  }
};

// Global keyboard listener setup
let keydownListenerAttached = false;

export const attachGlobalKeydownListener = () => {
  if (!keydownListenerAttached) {
    document.addEventListener("keydown", handleGlobalKeydown);
    keydownListenerAttached = true;
  }
};

export const detachGlobalKeydownListener = () => {
  if (keydownListenerAttached) {
    document.removeEventListener("keydown", handleGlobalKeydown);
    keydownListenerAttached = false;
  }
};

// Context-aware command filtering
export const $availableCommands = $commands.map((commands) => {
  // Filter commands based on current route/context
  const currentHash = window.location.hash;

  return commands
    .filter((command) => {
      // Always show navigation and system commands
      if (command.section === "Navigation" || command.section === "System") {
        return true;
      }

      // Show REST console commands only when in console
      if (command.section === "REST Console") {
        return currentHash.includes("/console");
      }

      // Show theme commands everywhere
      if (command.section === "Theme") {
        return true;
      }

      return true;
    })
    .sort((a, b) => (b.priority || 0) - (a.priority || 0));
});
