import { useMemo } from "react";
import { useUnit } from "effector-react";
import { IconSwitch, IconRefresh, IconCode, IconWand } from "@tabler/icons-react";
import { $method, $mode, resetDraft } from "../../state/consoleStore";
import type { ConsoleCommand } from "../types";
import type { HttpMethod } from "@/shared/api";

/**
 * Provides commands for builder actions (method switching, mode toggle, reset)
 * @returns Array of builder-related commands
 */
export function useBuilderCommands(): ConsoleCommand[] {
  const { method, mode, resetDraft: resetDraftEvent } = useUnit({
    method: $method,
    mode: $mode,
    resetDraft,
  });

  return useMemo(() => {
    const commands: ConsoleCommand[] = [];

    // Method switching commands
    const methods: HttpMethod[] = ["GET", "POST", "PUT", "DELETE", "PATCH"];
    for (const m of methods) {
      if (m !== method) {
        commands.push({
          id: `method-${m}`,
          label: `Switch to ${m}`,
          description: `Change HTTP method to ${m}`,
          category: "builder",
          badge: m,
          badgeColor: m === "GET" ? "blue" : m === "POST" ? "green" : "orange",
          keywords: ["method", "http", m.toLowerCase()],
          icon: <IconSwitch size={16} />,
          execute: (ctx) => {
            ctx.setMethod(m);
            ctx.closePalette();
            ctx.trackEvent?.("rest_console.command_palette.action", {
              action: "switch_method",
              method: m,
            });
          },
        });
      }
    }

    // Mode toggle command
    const targetMode = mode === "smart" ? "raw" : "smart";
    const modeIcon = mode === "smart" ? <IconCode size={16} /> : <IconWand size={16} />;

    commands.push({
      id: "toggle-mode",
      label: `Switch to ${targetMode === "smart" ? "Smart" : "Raw"} Mode`,
      description: targetMode === "smart" ? "Use autocomplete builder" : "Enter URLs manually",
      category: "builder",
      badge: targetMode === "smart" ? "Smart" : "Raw",
      badgeColor: targetMode === "smart" ? "violet" : "gray",
      keywords: ["mode", "builder", "raw", "smart"],
      icon: modeIcon,
      execute: (ctx) => {
        ctx.setMode(targetMode);
        ctx.closePalette();
        ctx.trackEvent?.("rest_console.command_palette.action", {
          action: "toggle_mode",
          mode: targetMode,
        });
      },
    });

    // Reset builder command
    commands.push({
      id: "reset-builder",
      label: "Reset Builder",
      description: "Clear all fields and start fresh",
      category: "builder",
      keywords: ["reset", "clear", "clean"],
      icon: <IconRefresh size={16} />,
      execute: (ctx) => {
        resetDraftEvent();
        ctx.closePalette();
        ctx.trackEvent?.("rest_console.command_palette.action", {
          action: "reset_builder",
        });
      },
    });

    return commands;
  }, [method, mode, resetDraftEvent]);
}
