import type { Command } from "../model/commands";
import { addCommand, removeCommand, updateCommand } from "../model/store";

export interface RegisterCommandOptions extends Omit<Command, "id"> {
  id: string;
}

/**
 * Register a new command with the command palette
 */
export const registerCommand = (options: RegisterCommandOptions): (() => void) => {
  const command: Command = {
    ...options,
    priority: options.priority ?? 50,
  };

  // Add the command
  addCommand(command);

  // Return unregister function
  return () => {
    removeCommand(command.id);
  };
};

/**
 * Register multiple commands at once
 */
export const registerCommands = (commands: RegisterCommandOptions[]): (() => void) => {
  const unregisterFns = commands.map((cmd) => registerCommand(cmd));

  return () => {
    unregisterFns.forEach((fn) => fn());
  };
};

/**
 * Update an existing command
 */
export const updateRegisteredCommand = (id: string, updates: Partial<Command>): void => {
  updateCommand({ id, updates });
};

/**
 * Hook for React components to register commands
 */
export const useCommandRegistration = () => {
  return {
    register: registerCommand,
    registerMultiple: registerCommands,
    update: updateRegisteredCommand,
  };
};

/**
 * Utility to create context-specific command registrations
 */
export class CommandRegistry {
  private unregisterFns: Array<() => void> = [];

  register(options: RegisterCommandOptions): void {
    const unregister = registerCommand(options);
    this.unregisterFns.push(unregister);
  }

  registerMultiple(commands: RegisterCommandOptions[]): void {
    const unregister = registerCommands(commands);
    this.unregisterFns.push(unregister);
  }

  unregisterAll(): void {
    this.unregisterFns.forEach((fn) => fn());
    this.unregisterFns = [];
  }
}

/**
 * Create a scoped command registry for components/features
 */
export const createCommandRegistry = (): CommandRegistry => {
  return new CommandRegistry();
};
