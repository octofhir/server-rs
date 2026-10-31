import { fhirClient, HttpError, sameOriginPath } from "@/shared/api/fhirClient";
import type { HttpResponse } from "@/shared/api/types";

export type ExportLevel = "system" | "patient" | "group";

export interface ExportRequest {
  level: ExportLevel;
  groupId?: string;
  types: string[];
  /** Only resources changed after this instant. */
  since?: Date | null;
}

export interface ImportInput {
  type: string;
  url: string;
}

export interface OutputFile {
  type: string;
  url: string;
  count?: number;
}

export type JobState = "queued" | "in-progress" | "completed" | "failed" | "cancelled" | "expired";

export interface JobStatus {
  state: JobState;
  /** 0..1 while running. */
  progress?: number;
  output: OutputFile[];
  errors: OutputFile[];
  transactionTime?: string;
  message?: string;
}

const ASYNC = { Accept: "application/fhir+json", Prefer: "respond-async" };

/** Job id from the `Content-Location` of a 202 (`…/_async-status/{id}`). */
function jobIdFrom(response: HttpResponse<unknown>): string {
  const location = response.headers["content-location"] ?? "";
  const id = location.split("/_async-status/")[1]?.split(/[/?]/)[0];
  if (!id) throw new Error("The server accepted the request but returned no status location.");
  return id;
}

export function exportPath({ level, groupId, types, since }: ExportRequest): string {
  const base =
    level === "group" ? `/fhir/Group/${encodeURIComponent(groupId ?? "")}/$export` : level === "patient" ? "/fhir/Patient/$export" : "/fhir/$export";
  const params = new URLSearchParams();
  if (types.length > 0) params.set("_type", types.join(","));
  if (since) params.set("_since", since.toISOString());
  const query = params.toString();
  return query ? `${base}?${query}` : base;
}

export async function startExport(request: ExportRequest): Promise<string> {
  const response = await fhirClient.rawRequest("GET", exportPath(request), undefined, { headers: ASYNC });
  return jobIdFrom(response);
}

export async function startImport(inputs: ImportInput[]): Promise<string> {
  const parameters = {
    resourceType: "Parameters",
    parameter: inputs.map((input) => ({
      name: "input",
      part: [
        { name: "type", valueString: input.type },
        { name: "url", valueUrl: input.url },
      ],
    })),
  };
  const response = await fhirClient.rawRequest("POST", "/fhir/$import", parameters, {
    headers: { ...ASYNC, "Content-Type": "application/fhir+json" },
  });
  return jobIdFrom(response);
}

function files(value: unknown): OutputFile[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) =>
    item && typeof item === "object" && typeof item.url === "string"
      ? [{ type: String(item.type ?? ""), url: item.url, count: typeof item.count === "number" ? item.count : undefined }]
      : []
  );
}

function readStatus(response: HttpResponse<unknown>): JobStatus {
  const body = (response.data && typeof response.data === "object" ? response.data : {}) as Record<string, unknown>;
  const message = typeof body.errorMessage === "string" ? body.errorMessage : undefined;
  if (response.status === 202) {
    const progress = typeof body.progress === "number" ? body.progress : undefined;
    return { state: body.status === "queued" ? "queued" : "in-progress", progress, output: [], errors: [] };
  }
  if (response.status === 410) return { state: "cancelled", output: [], errors: [] };
  if (response.status === 404) return { state: "expired", output: [], errors: [], message: "The server no longer has this job." };
  if (response.status >= 400) {
    return { state: "failed", output: [], errors: [], message: message ?? `HTTP ${response.status}` };
  }
  return {
    state: "completed",
    output: files(body.output),
    errors: files(body.error),
    transactionTime: typeof body.transactionTime === "string" ? body.transactionTime : undefined,
    message,
  };
}

export async function fetchJobStatus(id: string): Promise<JobStatus> {
  try {
    return readStatus(await fhirClient.rawRequest("GET", `/fhir/_async-status/${id}`));
  } catch (error) {
    if (error instanceof HttpError) return readStatus(error.response);
    throw error;
  }
}

/** A job as `GET /fhir/_async-status` lists it. */
export interface ServerJob {
  jobId: string;
  status: "queued" | "in_progress" | "completed" | "failed" | "cancelled";
  progress: number;
  requestType: string;
  requestMethod: string;
  requestUrl: string;
  createdAt: string;
  completedAt?: string | null;
  expiresAt: string;
  errorMessage?: string | null;
}

/** The caller's unexpired async jobs, newest first. */
export async function listJobs(): Promise<ServerJob[]> {
  const response = await fhirClient.rawRequest("GET", "/fhir/_async-status");
  const jobs = (response.data as { jobs?: unknown } | undefined)?.jobs;
  return Array.isArray(jobs) ? (jobs as ServerJob[]) : [];
}

/** `DELETE` stops a running job, or tells the server a finished job's files are no longer needed. */
export async function cancelJob(id: string): Promise<void> {
  await fhirClient.rawRequest("DELETE", `/fhir/_async-status/${id}`);
}

export async function downloadFile(file: OutputFile): Promise<void> {
  // The server's content negotiation rejects `Accept: application/fhir+ndjson` with 415.
  const response = await fhirClient.rawRequest("GET", sameOriginPath(file.url), undefined, {
    headers: { Accept: "*/*" },
  });
  const text = typeof response.data === "string" ? response.data : JSON.stringify(response.data);
  const blob = new Blob([text], { type: "application/fhir+ndjson" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = `${file.type || "export"}.ndjson`;
  link.click();
  // The click has started the download; the object URL is not needed after this tick.
  setTimeout(() => URL.revokeObjectURL(link.href), 0);
}
