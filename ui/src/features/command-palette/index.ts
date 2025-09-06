export type { RegisterCommandOptions } from "./lib/registerCommand";

export {
  CommandRegistry,
  createCommandRegistry,
  registerCommand,
  registerCommands,
  updateRegisteredCommand,
  useCommandRegistration,
} from "./lib/registerCommand";
export type { Command } from "./model/commands";

export {
  actionCommands,
  COMMAND_SECTIONS,
  commandToAction,
  defaultCommands,
  navigationCommands,
  restConsoleCommands,
  systemCommands,
} from "./model/commands";
export {
  $availableCommands,
  $commands,
  $commandsById,
  $isCommandPaletteOpen,
  addCommand,
  closeCommandPalette,
  openCommandPalette,
  removeCommand,
  resetCommands,
  setCommands,
  toggleCommandPalette,
  updateCommand,
} from "./model/store";
export {
  CommandPaletteProvider,
  CommandPaletteTrigger,
  useCommandPalette,
} from "./ui/CommandPalette";
