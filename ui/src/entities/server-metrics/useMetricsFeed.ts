import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { serverApi } from "@/shared/api";
import { metricsScraped, POLL_INTERVAL_MS } from "./model";
import { type MetricsSnapshot, toSnapshot } from "./prometheus";

/** Scrapes `/metrics` every couple of seconds while the tab is visible and not paused. */
export function useMetricsFeed(paused: boolean) {
  const query = useQuery<MetricsSnapshot>({
    queryKey: ["performance", "metrics"],
    queryFn: async () => toSnapshot(await serverApi.getMetricsText(), Date.now()),
    enabled: !paused,
    refetchInterval: POLL_INTERVAL_MS,
    // Every scrape is a new point; a cached one must never be replayed.
    gcTime: 0,
    staleTime: 0,
    structuralSharing: false,
    retry: false,
  });

  useEffect(() => {
    if (query.data) metricsScraped(query.data);
  }, [query.data]);

  return { error: query.error, isPending: query.isPending && !paused, retry: () => void query.refetch() };
}
