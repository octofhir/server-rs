import {
  ActionIcon,
  Button,
  Combobox,
  DataTable,
  type DataTableColumn,
  DatePicker,
  EmptyState,
  notify,
  SegmentedRadioGroup,
  Select,
  StatusBadge,
  type StatusTone,
  Switch,
  TextArea,
  TextInput,
} from "@octofhir/ui-kit";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { ChevronRight, Download, PackageOpen, Plus, Trash2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useResourceTypesCategorized } from "@/shared/api/hooks";
import { sameOriginPath } from "@/shared/api/fhirClient";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  type AssociatedData,
  cancelJob,
  downloadFile,
  type ExportLevel,
  exportParameters,
  exportPath,
  fetchJobStatus,
  fetchManifestPage,
  type ImportInput,
  type JobState,
  type JobStatus,
  listJobs,
  type OutputFile,
  type ServerJob,
  startExport,
  startImport,
} from "../lib/api";
import classes from "./BulkDataPage.module.css";
import { FileImport } from "./FileImport";
import { SubmitForm } from "./SubmitForm";

dayjs.extend(relativeTime);

const POLL_MS = 2000;
/** While nothing runs the list is refreshed rarely, to pick up jobs started elsewhere. */
const IDLE_POLL_MS = 15_000;
const JOBS_KEY = ["bulk-data", "jobs"] as const;

const KIND_LABELS: Record<string, string> = {
  bulk_export: "Export",
  bulk_import: "Import",
  bulk_submit: "Submit",
  bulk_submit_revert: "Take back",
  bulk_submit_status: "Submit status",
  viewdefinition_export: "View export",
};

/** Request types whose status URL returns a Bulk Data manifest with files. */
const MANIFEST_TYPES = new Set(["bulk_export", "viewdefinition_export", "bulk_submit_status"]);

const STATE_VIEW: Record<JobState, { label: string; tone: StatusTone }> = {
  queued: { label: "Queued", tone: "neutral" },
  "in-progress": { label: "Running", tone: "info" },
  completed: { label: "Completed", tone: "success" },
  failed: { label: "Failed", tone: "danger" },
  cancelled: { label: "Cancelled", tone: "neutral" },
  expired: { label: "Expired", tone: "neutral" },
};

const LEVELS: { value: ExportLevel; label: string }[] = [
  { value: "system", label: "Everything" },
  { value: "patient", label: "Patients" },
  { value: "group", label: "Group" },
];

function isRunning(status: JobStatus | undefined): boolean {
  return !status || status.state === "queued" || status.state === "in-progress";
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "The server rejected the request.";
}

interface JobRow {
  id: string;
  kind: string;
  /** e.g. `GET /fhir/Patient/$export?_type=Observation` */
  request: string;
  startedAt: string;
  status: JobStatus | undefined;
}

function listedStatus(job: ServerJob): JobStatus {
  const state: JobState =
    job.status === "in_progress" ? "in-progress" : job.status === "queued" ? "queued" : job.status;
  return { state, progress: job.progress, output: [], errors: [], deleted: [], message: job.errorMessage ?? undefined };
}

function requestLabel(job: ServerJob): string {
  try {
    return `${job.requestMethod} ${decodeURIComponent(sameOriginPath(job.requestUrl))}`;
  } catch {
    return `${job.requestMethod} ${sameOriginPath(job.requestUrl)}`;
  }
}

function summary(status: JobStatus | undefined): string {
  if (!status || status.state !== "completed") return "—";
  const resources = status.output.reduce((sum, file) => sum + (file.count ?? 0), 0);
  const fileWord = status.output.length === 1 ? "file" : "files";
  return `${status.output.length} ${fileWord} · ${resources.toLocaleString()} resources`;
}

const COLUMNS: DataTableColumn<JobRow>[] = [
  {
    id: "kind",
    header: "Job",
    width: 96,
    cell: (job) => <span className={classes.kind}>{KIND_LABELS[job.kind] ?? job.kind}</span>,
  },
  {
    id: "request",
    header: "Request",
    cell: (job) => <span className={classes.mono}>{job.request}</span>,
  },
  {
    id: "status",
    header: "Status",
    width: 140,
    cell: (job) => {
      const view = STATE_VIEW[job.status?.state ?? "queued"];
      const progress = job.status?.progress;
      return (
        <StatusBadge tone={view.tone}>
          {view.label}
          {job.status?.state === "in-progress" && progress != null ? ` · ${Math.round(progress * 100)}%` : ""}
        </StatusBadge>
      );
    },
  },
  {
    id: "output",
    header: "Output",
    width: 180,
    cell: (job) => <span className={classes.muted}>{summary(job.status)}</span>,
  },
  {
    id: "started",
    header: "Started",
    width: 120,
    cell: (job) => (
      <span className={classes.muted} title={new Date(job.startedAt).toLocaleString()}>
        {dayjs(job.startedAt).fromNow()}
      </span>
    ),
  },
];

const ASSOCIATED_DATA: { value: AssociatedData | "default"; label: string }[] = [
  { value: "default", label: "Server default" },
  { value: "LatestProvenanceResources", label: "Latest Provenance" },
  { value: "RelevantProvenanceResources", label: "All Provenance" },
];

/** Comma- or newline-separated input as a list. */
function list(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function ExportForm() {
  const { data: catalog } = useResourceTypesCategorized();
  const typeNames = useMemo(() => (catalog?.types ?? []).map((type) => type.name).sort(), [catalog]);
  const [level, setLevel] = useState<ExportLevel>("system");
  const [groupId, setGroupId] = useState("");
  const [types, setTypes] = useState<string[]>([]);
  const [since, setSince] = useState<Date | null>(null);
  const [until, setUntil] = useState<Date | null>(null);
  const [typeFilters, setTypeFilters] = useState("");
  const [elements, setElements] = useState("");
  const [patients, setPatients] = useState("");
  const [associatedData, setAssociatedData] = useState<AssociatedData>("");
  const [organizeByPatient, setOrganizeByPatient] = useState(false);
  const [partialManifests, setPartialManifests] = useState(false);
  const [pending, setPending] = useState(false);

  const request = {
    level,
    groupId: groupId.trim(),
    types,
    since,
    until,
    // A comma can be part of a search value, so filters go one per line.
    typeFilters: typeFilters
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean),
    elements: list(elements),
    patients: list(patients),
    associatedData,
    organizeByPatient,
    partialManifests,
  };
  const parameterCount = exportParameters(request).length;
  const blocked = level === "group" && !request.groupId;
  const queryClient = useQueryClient();

  const submit = async () => {
    setPending(true);
    try {
      await startExport(request);
      await queryClient.invalidateQueries({ queryKey: JOBS_KEY });
    } catch (error) {
      notify({ theme: "danger", title: "Export was not started", content: errorText(error) });
    } finally {
      setPending(false);
    }
  };

  return (
    <div className={classes.form}>
      <div className={classes.fields}>
        <SegmentedRadioGroup
          aria-label="What to export"
          value={level}
          onChange={(value) => setLevel(value as ExportLevel)}
          options={LEVELS}
        />
        {level === "group" && (
          <TextInput
            aria-label="Group id"
            placeholder="Group id"
            value={groupId}
            onChange={setGroupId}
            className={classes.groupInput}
          />
        )}
        <div className={classes.types}>
          <Combobox
            multiple
            aria-label="Resource types"
            placeholder={types.length === 0 ? "All resource types" : "Add type…"}
            data={typeNames}
            value={types}
            onChange={setTypes}
          />
        </div>
      </div>
      <div className={classes.fields}>
        <span className={classes.fieldLabel}>Changed</span>
        <div className={classes.since}>
          <DatePicker aria-label="Changed since" withTime value={since} onChange={setSince} />
        </div>
        <span className={classes.fieldLabel}>to</span>
        <div className={classes.since}>
          <DatePicker aria-label="Changed until" withTime value={until} onChange={setUntil} />
        </div>
        {level !== "system" && (
          <TextInput
            aria-label="Patient ids"
            placeholder="Patient ids, comma-separated"
            value={patients}
            onChange={setPatients}
            className={classes.urlInput}
          />
        )}
      </div>
      <div className={classes.fields}>
        <TextInput
          aria-label="Elements"
          placeholder="Elements: id, Patient.gender"
          value={elements}
          onChange={setElements}
          className={classes.urlInput}
        />
        <div className={classes.importType}>
          <Select
            aria-label="Provenance"
            value={associatedData || "default"}
            onChange={(value) => setAssociatedData(value === "default" ? "" : ((value ?? "") as AssociatedData))}
            data={ASSOCIATED_DATA}
          />
        </div>
      </div>
      <TextArea
        aria-label="Type filters"
        placeholder="Type filters, one per line: Observation?status=final"
        rows={2}
        value={typeFilters}
        onChange={setTypeFilters}
      />
      <div className={classes.fields}>
        <Switch label="Organize by Patient" checked={organizeByPatient} onChange={setOrganizeByPatient} />
        <Switch label="Partial manifests" checked={partialManifests} onChange={setPartialManifests} />
        <code className={classes.preview}>
          POST {decodeURIComponent(exportPath(request))} · {parameterCount} parameters
        </code>
        <Button
          className={classes.push}
          variant="filled"
          loading={pending}
          disabled={blocked}
          onClick={() => void submit()}
        >
          Start export
        </Button>
      </div>
    </div>
  );
}

function ImportForm() {
  const { data: catalog } = useResourceTypesCategorized();
  const typeNames = useMemo(() => (catalog?.types ?? []).map((type) => type.name).sort(), [catalog]);
  const [inputs, setInputs] = useState<ImportInput[]>([{ type: "", url: "" }]);
  const [pending, setPending] = useState(false);
  const queryClient = useQueryClient();

  const ready = inputs.filter((input) => input.type && /^https?:\/\//.test(input.url.trim()));
  const update = (index: number, patch: Partial<ImportInput>) =>
    setInputs((rows) => rows.map((row, at) => (at === index ? { ...row, ...patch } : row)));

  const submit = async () => {
    setPending(true);
    try {
      const sources = ready.map((input) => ({ type: input.type, url: input.url.trim() }));
      await startImport(sources);
      await queryClient.invalidateQueries({ queryKey: JOBS_KEY });
      setInputs([{ type: "", url: "" }]);
    } catch (error) {
      notify({ theme: "danger", title: "Import was not started", content: errorText(error) });
    } finally {
      setPending(false);
    }
  };

  return (
    <div className={classes.form}>
      {inputs.map((input, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity besides their position
        <div key={index} className={classes.fields}>
          <div className={classes.importType}>
            <Combobox
              aria-label="Resource type"
              placeholder="Type"
              data={typeNames}
              value={input.type || null}
              onChange={(value) => update(index, { type: value ?? "" })}
            />
          </div>
          <TextInput
            aria-label="NDJSON URL"
            placeholder="https://example.org/Patient.ndjson"
            value={input.url}
            onChange={(url) => update(index, { url })}
            className={classes.urlInput}
          />
          {inputs.length > 1 && (
            <ActionIcon
              variant="subtle"
              aria-label="Remove source"
              onClick={() => setInputs((rows) => rows.filter((_, at) => at !== index))}
            >
              <X size={14} />
            </ActionIcon>
          )}
        </div>
      ))}
      <div className={classes.fields}>
        <Button variant="subtle" onClick={() => setInputs((rows) => [...rows, { type: "", url: "" }])}>
          <Button.Icon side="left">
            <Plus size={14} />
          </Button.Icon>
          Add source
        </Button>
        <span className={classes.hint}>The server downloads each file itself, so the URLs must be reachable from it.</span>
        <Button
          className={classes.push}
          variant="filled"
          loading={pending}
          disabled={ready.length === 0}
          onClick={() => void submit()}
        >
          Start import
        </Button>
      </div>
    </div>
  );
}

function FileList({ files, onDownload }: { files: OutputFile[]; onDownload: (file: OutputFile) => void }) {
  return (
    <ul className={classes.files}>
      {files.map((file) => (
        <li key={file.url} className={classes.file}>
          <span className={classes.fileType}>{file.type || "Patient blocks"}</span>
          <span className={classes.muted}>{file.count != null ? `${file.count.toLocaleString()} resources` : ""}</span>
          {file.manifestUrl && <span className={classes.mono}>{file.manifestUrl}</span>}
          <Button className={classes.push} size="xs" variant="outline" onClick={() => onDownload(file)}>
            <Button.Icon side="left">
              <Download size={13} />
            </Button.Icon>
            NDJSON
          </Button>
        </li>
      ))}
    </ul>
  );
}

function JobDetail({ job, onClose, onCancel }: { job: JobRow; onClose: () => void; onCancel: () => void }) {
  // Pages after the first come from `link`; the first page is the status itself.
  const [pages, setPages] = useState<JobStatus[]>([]);
  const [loadingPage, setLoadingPage] = useState(false);
  const first = job.status;
  const status = pages.at(-1) ?? first;
  const pageNumber = pages.length + 1;
  const paged = pageNumber > 1 || status?.next != null;

  const download = (file: OutputFile) =>
    void downloadFile(file).catch((error: unknown) =>
      notify({ theme: "danger", title: `Could not download ${file.type}`, content: errorText(error) })
    );
  const openNext = async () => {
    if (!status?.next) return;
    setLoadingPage(true);
    try {
      const page = await fetchManifestPage(status.next);
      setPages((loaded) => [...loaded, page]);
    } catch (error) {
      notify({ theme: "danger", title: "Could not load the next page", content: errorText(error) });
    } finally {
      setLoadingPage(false);
    }
  };

  return (
    <section className={classes.panel} aria-label="Job details">
      <header className={classes.panelHeader}>
        <h2 className={classes.panelTitle}>{KIND_LABELS[job.kind] ?? job.kind}</h2>
        <code className={classes.mono}>{job.id}</code>
        <div className={classes.push} />
        {isRunning(first) && (
          <Button size="xs" variant="outline" onClick={onCancel}>
            Cancel job
          </Button>
        )}
        <ActionIcon variant="subtle" aria-label="Close details" onClick={onClose}>
          <X size={14} />
        </ActionIcon>
      </header>
      {status?.message && <p className={classes.error}>{status.message}</p>}
      {status?.state === "completed" && status.output.length === 0 && status.errors.length === 0 && (
        <p className={classes.muted}>The job finished without output files.</p>
      )}
      {isRunning(status) && (status?.output.length ?? 0) === 0 && (
        <p className={classes.muted}>Files appear here when they are ready.</p>
      )}
      {paged && (
        <div className={classes.fields}>
          <Button size="xs" variant="subtle" disabled={pages.length === 0} onClick={() => setPages((p) => p.slice(0, -1))}>
            Previous page
          </Button>
          <span className={classes.muted}>Manifest page {pageNumber}</span>
          <Button size="xs" variant="subtle" disabled={!status?.next} loading={loadingPage} onClick={() => void openNext()}>
            Next page
            <Button.Icon side="right">
              <ChevronRight size={13} />
            </Button.Icon>
          </Button>
        </div>
      )}
      {(status?.output.length ?? 0) > 0 && <FileList files={status?.output ?? []} onDownload={download} />}
      {(status?.deleted.length ?? 0) > 0 && (
        <>
          <h3 className={classes.panelTitle}>Deleted</h3>
          <FileList files={status?.deleted ?? []} onDownload={download} />
        </>
      )}
      {(status?.errors.length ?? 0) > 0 && (
        <>
          <h3 className={classes.panelTitle}>Messages</h3>
          <FileList files={status?.errors ?? []} onDownload={download} />
        </>
      )}
    </section>
  );
}

export function BulkDataPage() {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<"export" | "import" | "submit">("export");
  const [importSource, setImportSource] = useState<"file" | "urls">("file");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const list = useQuery({
    queryKey: JOBS_KEY,
    queryFn: listJobs,
    refetchInterval: (query) =>
      query.state.data?.some((job) => job.status === "queued" || job.status === "in_progress")
        ? POLL_MS
        : IDLE_POLL_MS,
  });
  const jobs = list.data ?? [];

  // Finished exports carry their file manifest in `_async-status/{id}`; it never changes.
  // A submission status job is done at once, but its manifest changes until the
  // submission closes, so that one is re-read like a running job.
  const manifestJobs = jobs.filter((job) => job.status === "completed" && MANIFEST_TYPES.has(job.requestType));
  const manifests = useQueries({
    queries: manifestJobs.map((job) => {
      const live = job.requestType === "bulk_submit_status";
      return {
        queryKey: ["bulk-data", "manifest", job.jobId],
        queryFn: () => fetchJobStatus(job.jobId),
        staleTime: live ? POLL_MS : Number.POSITIVE_INFINITY,
        refetchInterval: (query: { state: { data?: JobStatus } }) =>
          live && query.state.data?.state !== "completed" ? POLL_MS * 2 : false as const,
      };
    }),
  });
  const manifestById = new Map(manifestJobs.map((job, index) => [job.jobId, manifests[index]?.data]));

  const rows: JobRow[] = jobs.map((job) => ({
    id: job.jobId,
    kind: job.requestType,
    request: requestLabel(job),
    startedAt: job.createdAt,
    status: manifestById.get(job.jobId) ?? listedStatus(job),
  }));
  const selected = rows.find((row) => row.id === selectedId) ?? null;

  const remove = (job: JobRow) =>
    cancelJob(job.id)
      .then(() => {
        if (selectedId === job.id) setSelectedId(null);
        return queryClient.invalidateQueries({ queryKey: JOBS_KEY });
      })
      .catch((error: unknown) =>
        notify({ theme: "danger", title: "The server kept the job", content: errorText(error) })
      );

  return (
    <WorkspacePageLayout
      title="Bulk data"
      description="Export resources as NDJSON, import them from a file or from URLs, or receive a submission."
      className="page-enter"
    >
      <section className={classes.panel} aria-label="New job">
        <header className={classes.panelHeader}>
          <SegmentedRadioGroup
            aria-label="Job type"
            value={mode}
            onChange={(value) => setMode(value as "export" | "import" | "submit")}
            options={[
              { value: "export", label: "Export" },
              { value: "import", label: "Import" },
              { value: "submit", label: "Submit" },
            ]}
          />
        </header>
        {mode === "export" && <ExportForm />}
        {mode === "submit" && <SubmitForm />}
        {mode === "import" && (
          <>
            <SegmentedRadioGroup
              aria-label="Import from"
              size="sm"
              value={importSource}
              onChange={(value) => setImportSource(value === "urls" ? "urls" : "file")}
              options={[
                { value: "file", label: "A file" },
                { value: "urls", label: "URLs ($import)" },
              ]}
            />
            {importSource === "file" ? <FileImport /> : <ImportForm />}
          </>
        )}
      </section>

      <section className={classes.panel} aria-label="Jobs">
        <header className={classes.panelHeader}>
          <h2 className={classes.panelTitle}>Jobs</h2>
          <span className={classes.muted}>yours, kept until they expire</span>
        </header>
        <DataTable
          aria-label="Bulk data jobs"
          data={rows}
          loading={list.isPending}
          loadingRowCount={3}
          columns={COLUMNS}
          getRowId={(job) => job.id}
          activeRowId={selectedId ?? undefined}
          maxHeight="none"
          onRowClick={(job) => setSelectedId((id) => (id === job.id ? null : job.id))}
          rowMenu={(job) =>
            [
              [
                ...(job.status?.state === "completed"
                  ? job.status.output.map((file) => ({
                      text: `Download ${file.type}`,
                      iconStart: <Download size={14} />,
                      action: () => void downloadFile(file),
                    }))
                  : []),
                ...(isRunning(job.status)
                  ? [{ text: "Cancel job", iconStart: <X size={14} />, action: () => void remove(job) }]
                  : []),
              ],
              isRunning(job.status)
                ? []
                : [
                    {
                      text: "Delete job and files",
                      iconStart: <Trash2 size={14} />,
                      theme: "danger" as const,
                      action: () => void remove(job),
                    },
                  ],
            ].filter((group) => group.length > 0)
          }
          emptyState={
            <EmptyState
              image={<PackageOpen width={32} height={32} aria-hidden="true" />}
              title="No jobs yet"
              description="Start an export above; its files stay downloadable until the job expires."
            />
          }
        />
      </section>

      {selected && (
        <JobDetail
          key={selected.id}
          job={selected}
          onClose={() => setSelectedId(null)}
          onCancel={() => void remove(selected)}
        />
      )}
    </WorkspacePageLayout>
  );
}
