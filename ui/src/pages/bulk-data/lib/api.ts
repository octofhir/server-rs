import { fhirClient, HttpError, sameOriginPath } from "@/shared/api/fhirClient";
import type { HttpResponse } from "@/shared/api/types";

export type ExportLevel = "system" | "patient" | "group";

export type AssociatedData = "" | "LatestProvenanceResources" | "RelevantProvenanceResources";

export interface ExportRequest {
  level: ExportLevel;
  groupId?: string;
  types: string[];
  /** Only resources changed after this instant. */
  since?: Date | null;
  /** Only resources changed before this instant. */
  until?: Date | null;
  /** `Type?search` queries, one per line. */
  typeFilters?: string[];
  /** Root elements to keep: `gender` or `Patient.gender`. */
  elements?: string[];
  /** Patient ids, for Patient and Group exports. */
  patients?: string[];
  associatedData?: AssociatedData;
  organizeByPatient?: boolean;
  partialManifests?: boolean;
}

export interface ImportInput {
  type: string;
  url: string;
}

export interface OutputFile {
  type: string;
  url: string;
  count?: number;
  /** The submitted manifest an outcome file reports on. */
  manifestUrl?: string;
}

export type JobState = "queued" | "in-progress" | "completed" | "failed" | "cancelled" | "expired";

export interface JobStatus {
  state: JobState;
  /** 0..1 while running. */
  progress?: number;
  output: OutputFile[];
  errors: OutputFile[];
  /** Files of a submission's status manifest are in `errors`; deletions are listed apart. */
  deleted: OutputFile[];
  transactionTime?: string;
  message?: string;
  /** URL of the next manifest page, when the manifest is paged. */
  next?: string;
}

const ASYNC = { Accept: "application/fhir+json", Prefer: "respond-async" };

/** Job id from the `Content-Location` of a 202 (`…/_async-status/{id}`). */
function jobIdFrom(response: HttpResponse<unknown>): string {
  const location = response.headers["content-location"] ?? "";
  const id = location.split("/_async-status/")[1]?.split(/[/?]/)[0];
  if (!id) throw new Error("The server accepted the request but returned no status location.");
  return id;
}

export function exportPath({ level, groupId }: Pick<ExportRequest, "level" | "groupId">): string {
  if (level === "group") return `/fhir/Group/${encodeURIComponent(groupId ?? "")}/$export`;
  return level === "patient" ? "/fhir/Patient/$export" : "/fhir/$export";
}

/** Kick-off parameters as a Parameters resource: POST works in every IG version. */
export function exportParameters(request: ExportRequest): unknown[] {
  const parameter: unknown[] = [];
  for (const type of request.types) parameter.push({ name: "_type", valueString: type });
  if (request.since) parameter.push({ name: "_since", valueInstant: request.since.toISOString() });
  if (request.until) parameter.push({ name: "_until", valueInstant: request.until.toISOString() });
  for (const filter of request.typeFilters ?? []) parameter.push({ name: "_typeFilter", valueString: filter });
  for (const element of request.elements ?? []) parameter.push({ name: "_elements", valueString: element });
  if (request.level !== "system") {
    for (const id of request.patients ?? []) {
      parameter.push({ name: "patient", valueReference: { reference: `Patient/${id}` } });
    }
  }
  if (request.associatedData) {
    parameter.push({
      name: "includeAssociatedData",
      valueCoding: {
        system: "http://hl7.org/fhir/uv/bulkdata/CodeSystem/include-associated-data",
        code: request.associatedData,
      },
    });
  }
  if (request.organizeByPatient) parameter.push({ name: "organizeOutputBy", valueString: "Patient" });
  if (request.partialManifests) parameter.push({ name: "allowPartialManifests", valueBoolean: true });
  return parameter;
}

export async function startExport(request: ExportRequest): Promise<string> {
  const response = await fhirClient.rawRequest(
    "POST",
    exportPath(request),
    { resourceType: "Parameters", parameter: exportParameters(request) },
    { headers: { ...ASYNC, "Content-Type": "application/fhir+json" } }
  );
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
      ? [
          {
            type: String(item.type ?? ""),
            url: item.url,
            count: typeof item.count === "number" ? item.count : undefined,
            manifestUrl: typeof item.manifestUrl === "string" ? item.manifestUrl : undefined,
          },
        ]
      : []
  );
}

function nextPage(body: Record<string, unknown>): string | undefined {
  if (!Array.isArray(body.link)) return undefined;
  const link = body.link.find((item) => item && item.relation === "next" && typeof item.url === "string");
  return link?.url;
}

/** STU 3 names the messages `error`, STU 4 and status manifests name them `outcome`. */
function manifestFields(body: Record<string, unknown>) {
  return {
    output: files(body.output),
    errors: files(body.outcome ?? body.error),
    deleted: files(body.deleted),
    next: nextPage(body),
    transactionTime: typeof body.transactionTime === "string" ? body.transactionTime : undefined,
  };
}

function readStatus(response: HttpResponse<unknown>): JobStatus {
  const body = (response.data && typeof response.data === "object" ? response.data : {}) as Record<string, unknown>;
  const message = typeof body.errorMessage === "string" ? body.errorMessage : undefined;
  if (response.status === 202) {
    const progress = typeof body.progress === "number" ? body.progress : undefined;
    // A partial manifest: the files available so far.
    return { ...manifestFields(body), state: body.status === "queued" ? "queued" : "in-progress", progress };
  }
  if (response.status === 404) {
    return { state: "expired", output: [], errors: [], deleted: [], message: "The server no longer has this job." };
  }
  if (response.status >= 400) {
    return { state: "failed", output: [], errors: [], deleted: [], message: message ?? `HTTP ${response.status}` };
  }
  return { ...manifestFields(body), state: "completed", message };
}

export async function fetchJobStatus(id: string): Promise<JobStatus> {
  return fetchPath(`/fhir/_async-status/${id}`);
}

async function fetchPath(path: string): Promise<JobStatus> {
  try {
    return readStatus(await fhirClient.rawRequest("GET", path));
  } catch (error) {
    if (error instanceof HttpError) return readStatus(error.response);
    throw error;
  }
}

/** A manifest page reached through a `next` link. */
export async function fetchManifestPage(url: string): Promise<JobStatus> {
  return fetchPath(sameOriginPath(url));
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
  const response = await fhirClient.rawRequest("GET", sameOriginPath(file.url), undefined, {
    headers: { Accept: "application/fhir+ndjson" },
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

export interface SubmissionIdentity {
  submitterSystem: string;
  submitterValue: string;
  submissionId: string;
}

export interface SubmissionManifest {
  manifestUrl: string;
  fhirBaseUrl: string;
  /** `Name: value` lines sent with every manifest and file request. */
  fileHeaders: string[];
  replacesManifestUrl?: string;
}

export type SubmissionStatusCode = "completed" | "stopped";

function submissionParameters(identity: SubmissionIdentity): unknown[] {
  return [
    {
      name: "submitter",
      valueIdentifier: { system: identity.submitterSystem, value: identity.submitterValue },
    },
    { name: "submissionId", valueString: identity.submissionId },
  ];
}

function postParameters(path: string, parameter: unknown[], headers: Record<string, string>) {
  return fhirClient.rawRequest("POST", path, { resourceType: "Parameters", parameter }, {
    headers: { ...headers, "Content-Type": "application/fhir+json" },
  });
}

/** Hands a manifest to the server, which fetches and stores its files. */
export async function submitManifest(identity: SubmissionIdentity, manifest: SubmissionManifest): Promise<void> {
  const parameter = [
    ...submissionParameters(identity),
    { name: "manifestUrl", valueUrl: manifest.manifestUrl },
    { name: "fhirBaseUrl", valueUrl: manifest.fhirBaseUrl },
  ];
  if (manifest.replacesManifestUrl) {
    parameter.push({ name: "replacesManifestUrl", valueUrl: manifest.replacesManifestUrl });
  }
  for (const line of manifest.fileHeaders) {
    const colon = line.indexOf(":");
    if (colon < 1) throw new Error(`Not a header: ${line}`);
    parameter.push({
      name: "fileRequestHeader",
      part: [
        { name: "headerName", valueString: line.slice(0, colon).trim() },
        { name: "headerValue", valueString: line.slice(colon + 1).trim() },
      ],
    });
  }
  await postParameters("/fhir/$bulk-submit", parameter, { Accept: "application/fhir+json" });
}

/** Closes a submission: `completed`, or `stopped`, which takes its data back. */
export async function closeSubmission(identity: SubmissionIdentity, code: SubmissionStatusCode): Promise<void> {
  const parameter = [
    ...submissionParameters(identity),
    { name: "submissionStatus", valueCoding: { system: "http://hl7.org/fhir/event-status", code } },
  ];
  await postParameters("/fhir/$bulk-submit", parameter, { Accept: "application/fhir+json" });
}

/** Starts `$bulk-submit-status`; the job's polling URL returns the status manifest. */
export async function startSubmissionStatus(identity: SubmissionIdentity): Promise<string> {
  const response = await postParameters("/fhir/$bulk-submit-status", submissionParameters(identity), ASYNC);
  return jobIdFrom(response);
}
