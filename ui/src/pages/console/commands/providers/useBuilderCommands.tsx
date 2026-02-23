import { IconRefresh, IconSwitch } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import { useMemo } from "react";
import type { HttpMethod } from "@/shared/api";
import { $method, resetDraft } from "../../state/consoleStore";
import type { ConsoleCommand } from "../types";

/**
 * Provides commands for builder actions (method switching, reset)
 * @returns Array of builder-related commands
 */
export function useBuilderCommands(): ConsoleCommand[] {
  const { method, resetDraft: resetDraftEvent } = useUnit({
    method: $method,
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
  }, [method, resetDraftEvent]);
}
