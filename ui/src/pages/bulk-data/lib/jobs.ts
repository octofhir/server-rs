import { createEvent, createStore } from "effector";

const STORAGE_KEY = "octofhir-bulk-jobs";
const LIMIT = 50;

/**
 * A job started from this console. The server keeps no list of jobs per user, so the console
 * remembers the ids it started; their state is always read from `_async-status`.
 */
export interface TrackedJob {
  id: string;
  kind: "export" | "import";
  /** The request as typed, e.g. `GET /fhir/Patient/$export?_type=Observation`. */
  request: string;
  startedAt: string;
}

function load(): TrackedJob[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter(
          (job): job is TrackedJob =>
            typeof job?.id === "string" && typeof job?.request === "string" && (job.kind === "export" || job.kind === "import")
        )
      : [];
  } catch {
    return [];
  }
}

export const jobStarted = createEvent<TrackedJob>();
export const jobForgotten = createEvent<string>();

/** Newest first. */
export const $jobs = createStore<TrackedJob[]>(load())
  .on(jobStarted, (jobs, job) => [job, ...jobs.filter((entry) => entry.id !== job.id)].slice(0, LIMIT))
  .on(jobForgotten, (jobs, id) => jobs.filter((job) => job.id !== id));

$jobs.watch((jobs) => {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(jobs));
  } catch {
    // Storage may be unavailable (private mode); the list then lasts for this tab only.
  }
});
