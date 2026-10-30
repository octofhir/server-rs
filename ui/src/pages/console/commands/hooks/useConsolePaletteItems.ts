import type { CommandPaletteItem } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { useMemo } from "react";
import { paletteOpenChanged } from "@/shared/command-palette";
import {
  $method,
  $mode,
  $rawPath,
  setBody,
  setCustomHeaders,
  setMethod,
  setMode,
  setRawPath,
} from "../../state/consoleStore";
import { useBuilderCommands, useHistoryCommands } from "../providers";
import type { CommandContext } from "../types";

/** REST console commands (method switch, reset, recent requests) as global palette items. */
export function useConsolePaletteItems(): CommandPaletteItem[] {
  const units = useUnit({
    method: $method,
    mode: $mode,
    rawPath: $rawPath,
    setMethod,
    setRawPath,
    setBody,
    setMode,
    setCustomHeaders,
    closePalette: paletteOpenChanged,
  });
  const builderCommands = useBuilderCommands();
  const historyCommands = useHistoryCommands();

  return useMemo(() => {
    const context: CommandContext = {
      setMethod: units.setMethod,
      setRawPath: units.setRawPath,
      setBody: units.setBody,
      setMode: units.setMode,
      setCustomHeaders: units.setCustomHeaders,
      currentMethod: units.method,
      currentMode: units.mode,
      currentPath: units.rawPath,
      closePalette: () => units.closePalette(false),
    };
    return [...builderCommands, ...historyCommands]
      .filter((command) => command.isAvailable?.(context) ?? true)
      .map((command) => ({
        id: `console-${command.id}`,
        label: command.label,
        group: command.category === "history" ? "Recent requests" : "Request",
        icon: command.icon,
        hint: command.badge ?? command.description,
        keywords: command.keywords,
        onSelect: () => {
          void command.execute(context);
        },
      }));
  }, [units, builderCommands, historyCommands]);
}
