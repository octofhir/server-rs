import { Sparkline } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import type { CSSProperties } from "react";
import { formatValue, lastValue, metricsApi } from "@/entities/metrics";
import { fhirClient } from "@/shared/api";
import { useSettings } from "@/shared/api/hooks/useSystemQueries";
import classes from "./DashboardWorkspace.module.css";

/** The overview shows the last ten minutes of load, from Prometheus (all instances). */
const TREND_SECONDS = 600;
const THROUGHPUT = "sum(rate(octofhir_http_requests_total[$__rate_interval]))";
const P95 =
  "histogram_quantile(0.95, sum by (le) (rate(octofhir_http_request_duration_seconds_bucket[$__rate_interval])))";

async function loadTrend() {
  const end = Math.floor(Date.now() / 1000);
  const [throughput, p95] = await Promise.all([
    metricsApi.queryRange({ query: THROUGHPUT, start: end - TREND_SECONDS, end }),
    metricsApi.queryRange({ query: P95, start: end - TREND_SECONDS, end }),
  ]);
  return {
    trend: throughput.series[0]?.points.map(([, value]) => value) ?? [],
    current: lastValue(throughput.series),
    p95: lastValue(p95.series),
  };
}

interface ListedJob {
  status: "queued" | "in_progress" | "completed" | "failed" | "cancelled";
}

async function listJobs(): Promise<ListedJob[]> {
  const response = await fhirClient.rawRequest("GET", "/fhir/_async-status");
  const jobs = (response.data as { jobs?: unknown } | undefined)?.jobs;
  return Array.isArray(jobs) ? (jobs as ListedJob[]) : [];
}

async function subscriptionStatuses(): Promise<string[]> {
  const bundle = await fhirClient.search("Subscription", { _count: 500, _elements: "status" });
  return (bundle.entry ?? []).map((entry) => String((entry.resource as { status?: string } | undefined)?.status ?? ""));
}

/** Live load, background jobs and subscriptions, each a step into its own page. */
export function ActivityStrip({ onNavigate }: { onNavigate: (href: string) => void }) {
  const settings = useSettings();
  const metricsEnabled = settings.data?.features.metrics === true;
  const load = useQuery({
    queryKey: ["dashboard", "throughput"],
    queryFn: loadTrend,
    enabled: metricsEnabled,
    refetchInterval: 15_000,
    retry: false,
  });

  const jobs = useQuery({ queryKey: ["dashboard", "jobs"], queryFn: listJobs, refetchInterval: 10_000 });
  const running = jobs.data?.filter((job) => job.status === "queued" || job.status === "in_progress").length ?? 0;
  const failedJobs = jobs.data?.filter((job) => job.status === "failed").length ?? 0;

  const subscriptions = useQuery({
    queryKey: ["dashboard", "subscriptions"],
    queryFn: subscriptionStatuses,
    refetchInterval: 30_000,
  });
  const active = subscriptions.data?.filter((status) => status === "active").length ?? 0;
  const inError = subscriptions.data?.filter((status) => status === "error").length ?? 0;

  return (
    <section
      className={`${classes.status} ${classes.activity}`}
      style={{ "--cells": metricsEnabled ? 3 : 2 } as CSSProperties}
      aria-label="Activity"
    >
      {metricsEnabled ? (
        <button type="button" className={`${classes.cell} ${classes.cellButton}`} onClick={() => onNavigate("/performance")}>
          <span className={classes.label}>Throughput</span>
          <span className={classes.value}>{load.data ? formatValue(load.data.current ?? 0, "reqps") : "—"}</span>
          <span className={classes.caption}>
            {load.isError
              ? "Prometheus unreachable"
              : load.data?.p95 == null
                ? "Idle"
                : `${formatValue(load.data.p95, "s")} p95 latency`}
          </span>
          <Sparkline values={load.data?.trend ?? []} height={18} />
        </button>
      ) : null}
      <button type="button" className={`${classes.cell} ${classes.cellButton}`} onClick={() => onNavigate("/bulk")}>
        <span className={classes.label}>Bulk jobs</span>
        <span className={classes.value}>
          {running > 0 && <span className={classes.dot} data-tone="warning" />}
          {jobs.isError ? "—" : running > 0 ? `${running} running` : (jobs.data?.length ?? 0).toLocaleString()}
        </span>
        <span className={classes.caption}>
          {jobs.data?.length === 0 ? "No exports or imports" : failedJobs > 0 ? `${failedJobs} failed` : "Kept until they expire"}
        </span>
      </button>
      <button type="button" className={`${classes.cell} ${classes.cellButton}`} onClick={() => onNavigate("/subscriptions")}>
        <span className={classes.label}>Subscriptions</span>
        <span className={classes.value}>
          {inError > 0 && <span className={classes.dot} data-tone="danger" />}
          {subscriptions.isError ? "—" : `${active} active`}
        </span>
        <span className={classes.caption}>
          {inError > 0 ? `${inError} in error` : (subscriptions.data?.length ?? 0) === 0 ? "None yet" : "None in error"}
        </span>
      </button>
    </section>
  );
}
