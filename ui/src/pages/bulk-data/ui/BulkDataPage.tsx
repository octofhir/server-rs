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
  StatusBadge,
  type StatusTone,
  TextInput,
} from "@octofhir/ui-kit";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { Download, PackageOpen, Plus, Trash2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useResourceTypesCategorized } from "@/shared/api/hooks";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  cancelJob,
  downloadFile,
  type ExportLevel,
  exportPath,
  fetchJobStatus,
  type ImportInput,
  type JobState,
  type JobStatus,
  listJobs,
  localPath,
  type ServerJob,
  startExport,
  startImport,
} from "../lib/api";
import classes from "./BulkDataPage.module.css";

dayjs.extend(relativeTime);

const POLL_MS = 2000;
/** While nothing runs the list is refreshed rarely, to pick up jobs started elsewhere. */
const IDLE_POLL_MS = 15_000;
const JOBS_KEY = ["bulk-data", "jobs"] as const;

const KIND_LABELS: Record<string, string> = {
  bulk_export: "Export",
  bulk_import: "Import",
  viewdefinition_export: "View export",
};

/** Request types whose result is a Bulk Data manifest with output files. */
const MANIFEST_TYPES = new Set(["bulk_export", "viewdefinition_export"]);

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
  return { state, progress: job.progress, output: [], errors: [], message: job.errorMessage ?? undefined };
}

function requestLabel(job: ServerJob): string {
  try {
    return `${job.requestMethod} ${decodeURIComponent(localPath(job.requestUrl))}`;
  } catch {
    return `${job.requestMethod} ${localPath(job.requestUrl)}`;
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

function ExportForm() {
  const { data: catalog } = useResourceTypesCategorized();
  const typeNames = useMemo(() => (catalog?.types ?? []).map((type) => type.name).sort(), [catalog]);
  const [level, setLevel] = useState<ExportLevel>("system");
  const [groupId, setGroupId] = useState("");
  const [types, setTypes] = useState<string[]>([]);
  const [since, setSince] = useState<Date | null>(null);
  const [pending, setPending] = useState(false);

  const request = { level, groupId: groupId.trim(), types, since };
  // Shown and remembered decoded (`_type=Patient,Observation`), sent encoded.
  const path = decodeURIComponent(exportPath(request));
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
        <div className={classes.since}>
          <DatePicker aria-label="Changed since" withTime value={since} onChange={setSince} />
        </div>
        <Button variant="filled" loading={pending} disabled={blocked} onClick={() => void submit()}>
          Start export
        </Button>
      </div>
      <code className={classes.preview}>GET {path}</code>
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

function JobDetail({ job, onClose }: { job: JobRow; onClose: () => void }) {
  const status = job.status;
  const download = (file: { type: string; url: string }) =>
    downloadFile(file).catch((error: unknown) =>
      notify({ theme: "danger", title: `Could not download ${file.type}`, content: errorText(error) })
    );

  return (
    <section className={classes.panel} aria-label="Job details">
      <header className={classes.panelHeader}>
        <h2 className={classes.panelTitle}>{job.kind === "export" ? "Export" : "Import"}</h2>
        <code className={classes.mono}>{job.id}</code>
        <ActionIcon className={classes.push} variant="subtle" aria-label="Close details" onClick={onClose}>
          <X size={14} />
        </ActionIcon>
      </header>
      {status?.message && <p className={classes.error}>{status.message}</p>}
      {status?.state === "completed" && status.output.length === 0 && (
        <p className={classes.muted}>The job finished without output files.</p>
      )}
      {isRunning(status) && <p className={classes.muted}>Files appear here when the job completes.</p>}
      {(status?.output.length ?? 0) > 0 && (
        <ul className={classes.files}>
          {status?.output.map((file) => (
            <li key={file.url} className={classes.file}>
              <span className={classes.fileType}>{file.type}</span>
              <span className={classes.muted}>{file.count != null ? `${file.count.toLocaleString()} resources` : ""}</span>
              <Button className={classes.push} size="xs" variant="outline" onClick={() => void download(file)}>
                <Button.Icon side="left">
                  <Download size={13} />
                </Button.Icon>
                NDJSON
              </Button>
            </li>
          ))}
        </ul>
      )}
      {(status?.errors.length ?? 0) > 0 && (
        <ul className={classes.files}>
          {status?.errors.map((file) => (
            <li key={file.url} className={classes.file}>
              <span className={classes.fileType}>OperationOutcome</span>
              <span className={classes.error}>{file.count ?? ""} issues</span>
              <Button className={classes.push} size="xs" variant="outline" onClick={() => void download(file)}>
                Errors
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function BulkDataPage() {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<"export" | "import">("export");
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
  const manifestJobs = jobs.filter((job) => job.status === "completed" && MANIFEST_TYPES.has(job.requestType));
  const manifests = useQueries({
    queries: manifestJobs.map((job) => ({
      queryKey: ["bulk-data", "manifest", job.jobId],
      queryFn: () => fetchJobStatus(job.jobId),
      staleTime: Number.POSITIVE_INFINITY,
    })),
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
      description="Export resources as NDJSON or import them from URLs, as background jobs."
      className="page-enter"
    >
      <section className={classes.panel} aria-label="New job">
        <header className={classes.panelHeader}>
          <SegmentedRadioGroup
            aria-label="Job type"
            value={mode}
            onChange={(value) => setMode(value === "import" ? "import" : "export")}
            options={[
              { value: "export", label: "Export" },
              { value: "import", label: "Import" },
            ]}
          />
        </header>
        {mode === "export" ? <ExportForm /> : <ImportForm />}
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

      {selected && <JobDetail job={selected} onClose={() => setSelectedId(null)} />}
    </WorkspacePageLayout>
  );
}
