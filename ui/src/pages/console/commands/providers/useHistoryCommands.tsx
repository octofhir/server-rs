import { IconClock, IconPinFilled } from "@tabler/icons-react";
import { useMemo } from "react";
import type { HttpMethod } from "@/shared/api";
import { useHistory } from "../../hooks/useHistory";
import type { ConsoleCommand } from "../types";

/**
 * Format time ago from ISO timestamp
 * @param isoTimestamp - ISO 8601 timestamp string
 * @returns Human-readable time ago string (e.g., "2 minutes ago")
 */
function formatTimeAgo(isoTimestamp: string): string {
  const now = Date.now();
  const then = new Date(isoTimestamp).getTime();
  const diffMs = now - then;
  const diffSeconds = Math.floor(diffMs / 1000);
  const diffMinutes = Math.floor(diffSeconds / 60);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSeconds < 60) {
    return "Just now";
  }
  if (diffMinutes < 60) {
    return `${diffMinutes} minute${diffMinutes === 1 ? "" : "s"} ago`;
  }
  if (diffHours < 24) {
    return `${diffHours} hour${diffHours === 1 ? "" : "s"} ago`;
  }
  return `${diffDays} day${diffDays === 1 ? "" : "s"} ago`;
}

/**
 * Provides commands from request history
 * Converts recent history entries into executable commands
 *
 * @returns Array of history-based commands (up to 20 most recent)
 */
export function useHistoryCommands(): ConsoleCommand[] {
  const { entries, isLoading } = useHistory();

  return useMemo(() => {
    if (isLoading || !entries) {
      return [];
    }

    // Limit to 20 most recent entries
    return entries.slice(0, 20).map((entry) => {
      const hasResponse = entry.responseStatus !== undefined;
      const isSuccess = hasResponse && entry.responseStatus >= 200 && entry.responseStatus < 300;
      const isError = hasResponse && entry.responseStatus >= 400;

      // Determine badge color based on response status
      let badgeColor = "gray";
      if (isSuccess) badgeColor = "green";
      else if (isError) badgeColor = "red";
      else if (entry.responseStatus && entry.responseStatus >= 300) badgeColor = "yellow";

      // Build description with timing info
      const timeAgo = formatTimeAgo(entry.requestedAt);
      const duration = entry.responseDurationMs ? ` · ${entry.responseDurationMs}ms` : "";
      const description = `${timeAgo}${duration}`;

      return {
        id: `history-${entry.id}`,
        label: `${entry.method} ${entry.path}`,
        description,
        category: "history" as const,
        badge: entry.responseStatus?.toString() || entry.responseStatusText,
        badgeColor,
        keywords: [
          entry.method.toLowerCase(),
          entry.path,
          entry.resourceType || "",
          ...(entry.tags || []),
        ],
        icon: entry.isPinned ? <IconPinFilled size={16} /> : <IconClock size={16} />,
        execute: (ctx) => {
          // Always restore to "pro" mode (the only active mode now)
          ctx.setMode("pro");
          ctx.setMethod(entry.method as HttpMethod);
          ctx.setRawPath(entry.path);

          if (entry.body) {
            ctx.setBody(entry.body);
          }

          if (entry.headers && Object.keys(entry.headers).length > 0) {
            ctx.setCustomHeaders(entry.headers);
          }

          ctx.closePalette();
          ctx.trackEvent?.("rest_console.command_palette.action", {
            action: "restore_history",
            method: entry.method,
            has_body: !!entry.body,
            has_headers: !!entry.headers,
            is_pinned: entry.isPinned,
          });
        },
        isAvailable: () => true,
      };
    });
  }, [entries, isLoading]);
}
