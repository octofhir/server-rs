import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { type InstantSample, metricsApi, type RangeResult, type Variables } from "./api";
import type { Panel } from "./dashboard";

export const metricsKeys = {
  all: ["metrics"] as const,
  status: () => [...metricsKeys.all, "status"] as const,
  catalog: () => [...metricsKeys.all, "catalog"] as const,
  labels: (metric?: string) => [...metricsKeys.all, "labels", metric ?? ""] as const,
  values: (label: string, metric?: string) => [...metricsKeys.all, "values", label, metric ?? ""] as const,
  builtin: () => [...metricsKeys.all, "builtin"] as const,
};

export function useMetricsStatus(enabled = true) {
  return useQuery({
    queryKey: metricsKeys.status(),
    queryFn: metricsApi.status,
    enabled,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });
}

/** Metric names and help, for completions; changes rarely. */
export function useMetricCatalog(enabled = true) {
  return useQuery({
    queryKey: metricsKeys.catalog(),
    queryFn: metricsApi.metrics,
    enabled,
    staleTime: 5 * 60_000,
  });
}

export function useLabelValues(label: string, metric?: string, enabled = true) {
  return useQuery({
    queryKey: metricsKeys.values(label, metric),
    queryFn: () => metricsApi.labelValues(label, metric),
    enabled,
    staleTime: 30_000,
  });
}

export function useBuiltinDashboards(enabled = true) {
  return useQuery({
    queryKey: metricsKeys.builtin(),
    queryFn: metricsApi.builtinDashboards,
    enabled,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export interface PanelData {
  /** Timeseries and stat panels: one range result per query. */
  ranges?: { refId: string; result: RangeResult }[];
  /** Table panels: one instant result per query. */
  instants?: { refId: string; samples: InstantSample[] }[];
}

/**
 * Data for one panel over `[end - range, end]`. `end` moves with each refresh tick, so the query
 * key changes and the panel re-fetches; the previous data stays on screen meanwhile.
 */
export function usePanelData(panel: Panel, range: number, end: number, variables: Variables, enabled = true) {
  const targets = panel.targets.filter((target) => target.expr.trim() !== "");
  return useQuery<PanelData>({
    queryKey: [...metricsKeys.all, "panel", panel.type, targets, range, end, variables],
    enabled: enabled && targets.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
    queryFn: async () => {
      if (panel.type === "table") {
        const instants = await Promise.all(
          targets.map(async (target) => ({
            refId: target.refId,
            samples: await metricsApi.query({ query: target.expr, time: end, range, variables }),
          }))
        );
        return { instants };
      }
      const ranges = await Promise.all(
        targets.map(async (target) => ({
          refId: target.refId,
          result: await metricsApi.queryRange({ query: target.expr, start: end - range, end, variables }),
        }))
      );
      return { ranges };
    },
  });
}
