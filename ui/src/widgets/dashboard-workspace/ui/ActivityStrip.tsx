import { Sparkline } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { useUnit } from "effector-react";
import { useMemo } from "react";
import { $snapshots, intervalSeries, useMetricsFeed } from "@/entities/server-metrics";
import { fhirClient } from "@/shared/api";
import classes from "./DashboardWorkspace.module.css";

/** The overview shows the last two minutes of load. */
const TREND_POINTS = 60;

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

function rate(total: number): string {
  return total >= 100 ? Math.round(total).toLocaleString() : total.toFixed(total >= 10 ? 1 : 2).replace(/\.?0+$/, "") || "0";
}

/** Live load, background jobs and subscriptions, each a step into its own page. */
export function ActivityStrip({ onNavigate }: { onNavigate: (href: string) => void }) {
  useMetricsFeed(false);
  const snapshots = useUnit($snapshots);
  const intervals = useMemo(() => intervalSeries(snapshots.slice(-(TREND_POINTS + 1))), [snapshots]);
  const totals = intervals.map((point) => (point.rate ? point.rate.ok + point.rate.client + point.rate.server : null));
  const current = totals.at(-1);
  const mean = intervals.at(-1)?.meanMs;

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
    <section className={`${classes.status} ${classes.activity}`} aria-label="Activity">
      <button type="button" className={`${classes.cell} ${classes.cellButton}`} onClick={() => onNavigate("/performance")}>
        <span className={classes.label}>Throughput</span>
        <span className={classes.value}>{current == null ? "—" : `${rate(current)} req/s`}</span>
        <span className={classes.caption}>{mean == null ? "Idle" : `${mean.toFixed(mean < 10 ? 2 : 1)} ms mean latency`}</span>
        <Sparkline values={totals} height={18} />
      </button>
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
