import {
  Button,
  Combobox,
  DataTable,
  type DataTableColumn,
  EmptyState,
  Progress,
  ReferenceLink,
  SegmentedRadioGroup,
  StatusBadge,
  type StatusTone,
  TextInput,
  useDebouncedValue,
} from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { usePackages, useResourceTypesCategorized } from "@/shared/api/hooks";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  groupIssues,
  type Issue,
  type IssueGroup,
  type ProfileHit,
  profileType,
  type ResourceResult,
  runValidation,
  searchProfiles,
} from "../lib/run";
import classes from "./ValidationPage.module.css";

const SAMPLES = ["100", "500", "1000", "5000"];

/** Results are buffered and painted at this rate so a fast run doesn't re-render per resource. */
const FLUSH_MS = 200;

const SEVERITY_TONE: Record<Issue["severity"], StatusTone> = {
  fatal: "danger",
  error: "danger",
  warning: "warning",
  information: "info",
};

interface RunState {
  type: string;
  fetched: number;
  results: ResourceResult[];
  startedAt: number;
  finishedAt?: number;
  cancelled?: boolean;
  error?: string;
}

function rate(run: RunState): number {
  const seconds = ((run.finishedAt ?? Date.now()) - run.startedAt) / 1000;
  return seconds > 0 ? run.results.length / seconds : 0;
}

function share(part: number, whole: number): string {
  return whole === 0 ? "—" : `${Math.round((part / whole) * 1000) / 10}%`;
}

export function ValidationPage() {
  const navigate = useNavigate();
  const { data: catalog } = useResourceTypesCategorized();
  const { data: packages } = usePackages();
  const typeNames = useMemo(
    () => (catalog?.types ?? []).filter((type) => type.category !== "system").map((type) => type.name).sort(),
    [catalog]
  );

  const [type, setType] = useState<string | null>("Patient");
  const [profileQuery, setProfileQuery] = useState("");
  const [profile, setProfile] = useState<ProfileHit | null>(null);
  const [filter, setFilter] = useState("");
  const [sample, setSample] = useState("500");
  const [run, setRun] = useState<RunState | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<string | null>(null);
  const abort = useRef<AbortController | null>(null);
  const buffer = useRef<ResourceResult[]>([]);

  const debouncedProfile = useDebouncedValue(profileQuery, 250);
  const profiles = useQuery({
    queryKey: ["validation", "profiles", debouncedProfile, packages?.packages.length],
    queryFn: () => searchProfiles(packages?.packages ?? [], debouncedProfile),
    enabled: packages != null,
    placeholderData: (previous) => previous,
  });
  const profileOptions = (profiles.data ?? []).map((hit) => ({
    value: hit.url,
    label: `${hit.name} · ${hit.packageName}`,
    textValue: hit.name,
  }));

  // Stop a run when the page goes away.
  useEffect(() => () => abort.current?.abort(), []);

  const running = run != null && run.finishedAt == null;

  const start = async () => {
    if (!type) return;
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;
    buffer.current = [];
    setSelectedGroup(null);
    const startedAt = Date.now();
    setRun({ type, fetched: 0, results: [], startedAt });

    const flush = window.setInterval(() => {
      const pending = buffer.current;
      if (pending.length === 0) return;
      buffer.current = [];
      setRun((current) => (current ? { ...current, results: [...current.results, ...pending] } : current));
    }, FLUSH_MS);

    let error: string | undefined;
    try {
      await runValidation({
        type,
        profile: profile?.url,
        query: filter,
        limit: Number(sample),
        signal: controller.signal,
        onFetched: (fetched) => setRun((current) => (current ? { ...current, fetched } : current)),
        onResult: (result) => buffer.current.push(result),
      });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : "The run failed.";
    } finally {
      window.clearInterval(flush);
      const pending = buffer.current;
      buffer.current = [];
      setRun((current) =>
        current
          ? {
              ...current,
              results: [...current.results, ...pending],
              finishedAt: Date.now(),
              cancelled: controller.signal.aborted,
              error,
            }
          : current
      );
    }
  };

  const results = run?.results ?? [];
  const groups = useMemo(() => groupIssues(results), [results]);
  const withErrors = results.filter((result) =>
    result.issues.some((issue) => issue.severity === "error" || issue.severity === "fatal")
  ).length;
  const withWarnings = results.filter(
    (result) =>
      result.issues.length > 0 && result.issues.every((issue) => issue.severity === "warning")
  ).length;
  const valid = results.length - withErrors;
  const selected = groups.find((group) => group.key === selectedGroup) ?? null;

  const pickProfile = async (url: string | null) => {
    const hit = profiles.data?.find((entry) => entry.url === url) ?? null;
    setProfile(hit);
    if (hit) {
      const constrained = await profileType(hit).catch(() => undefined);
      if (constrained) setType(constrained);
    }
  };

  const columns: DataTableColumn<IssueGroup>[] = [
    {
      id: "severity",
      header: "Severity",
      width: 96,
      cell: (group) => <StatusBadge tone={SEVERITY_TONE[group.severity]}>{group.severity}</StatusBadge>,
    },
    {
      id: "message",
      header: "Issue",
      cell: (group) => <span className={classes.message}>{group.message}</span>,
    },
    {
      id: "path",
      header: "Path",
      width: 240,
      cell: (group) => <span className={classes.mono}>{group.expression ?? "—"}</span>,
    },
    {
      id: "resources",
      header: "Resources",
      width: 110,
      align: "right",
      cell: (group) => (
        <span className={classes.mono}>
          {group.resourceIds.length.toLocaleString()} · {share(group.resourceIds.length, results.length)}
        </span>
      ),
    },
  ];

  const target = run ? Math.min(Number(sample), Math.max(run.fetched, results.length)) : 0;
  const query = profile ? `?profile=${profile.url}` : "";

  return (
    <WorkspacePageLayout
      title="Validation"
      description="Validate stored resources against the base specification or a profile, in bulk."
      className="page-enter"
    >
      <section className={classes.panel} aria-label="Run settings">
        <div className={classes.fields}>
          <div className={classes.type}>
            <Combobox aria-label="Resource type" placeholder="Resource type" data={typeNames} value={type} onChange={setType} />
          </div>
          <div className={classes.profile}>
            <Combobox
              aria-label="Profile"
              placeholder="Base specification (no profile)"
              data={profileOptions}
              value={profile?.url ?? null}
              onChange={(url) => void pickProfile(url)}
              onInputChange={setProfileQuery}
              filter="server"
              loading={profiles.isFetching}
              clearable
              emptyMessage="No resource profiles match"
            />
          </div>
          <TextInput
            className={classes.filter}
            aria-label="Search filter"
            placeholder="_tag=demo-data"
            leftSection={<span className={classes.prefix}>?</span>}
            value={filter}
            onChange={setFilter}
          />
          <SegmentedRadioGroup aria-label="Sample size" value={sample} onChange={setSample} options={SAMPLES.map((value) => ({ value, label: value }))} />
          {running ? (
            <Button variant="outline" onClick={() => abort.current?.abort()}>
              Cancel
            </Button>
          ) : (
            <Button variant="filled" disabled={!type} onClick={() => void start()}>
              Validate
            </Button>
          )}
        </div>
        <code className={classes.preview}>
          POST /fhir/{type ?? "…"}/$validate{query} · up to {Number(sample).toLocaleString()} resources
          {filter.trim() ? ` matching ?${filter.trim().replace(/^\?/, "")}` : ""}
        </code>
      </section>

      {run == null ? (
        <EmptyState
          size="promo"
          image={<ShieldCheck width={32} height={32} aria-hidden="true" />}
          title="Check how conformant your data is"
          description="Pick a type and, optionally, a profile. Issues are grouped by message and path so the systematic ones stand out."
        />
      ) : (
        <>
          <section className={classes.stats} aria-label="Summary">
            <div className={classes.cell}>
              <span className={classes.label}>Validated</span>
              <span className={classes.value}>{results.length.toLocaleString()}</span>
              <span className={classes.caption}>
                {running ? `of ${target.toLocaleString()} read so far` : run.cancelled ? "Cancelled" : `${run.type} resources`}
              </span>
            </div>
            <div className={classes.cell}>
              <span className={classes.label}>Valid</span>
              <span className={classes.value}>{share(valid, results.length)}</span>
              <span className={classes.caption}>{valid.toLocaleString()} without errors</span>
            </div>
            <div className={classes.cell}>
              <span className={classes.label}>With errors</span>
              <span className={classes.value}>{withErrors.toLocaleString()}</span>
              <span className={classes.caption}>{withWarnings.toLocaleString()} more with warnings only</span>
            </div>
            <div className={classes.cell}>
              <span className={classes.label}>Throughput</span>
              <span className={classes.value}>{Math.round(rate(run)).toLocaleString()}/s</span>
              <span className={classes.caption}>
                {(((run.finishedAt ?? Date.now()) - run.startedAt) / 1000).toFixed(1)} s · 6 parallel requests
              </span>
            </div>
          </section>
          {running && <Progress value={target > 0 ? (results.length / target) * 100 : 0} size="xs" />}
          {run.error && <p className={classes.error}>{run.error}</p>}

          <section className={classes.panel} aria-label="Issues">
            <header className={classes.panelHeader}>
              <h2 className={classes.panelTitle}>Issues</h2>
              <span className={classes.caption}>{groups.length.toLocaleString()} distinct, grouped by message and path</span>
            </header>
            <DataTable
              aria-label="Validation issues"
              data={groups}
              columns={columns}
              getRowId={(group) => group.key}
              activeRowId={selectedGroup ?? undefined}
              onRowClick={(group) => setSelectedGroup((key) => (key === group.key ? null : group.key))}
              maxHeight={440}
              emptyState={running ? "No issues so far." : "No issues: every resource is valid."}
            />
          </section>

          {selected && (
            <section className={classes.panel} aria-label="Affected resources">
              <header className={classes.panelHeader}>
                <h2 className={classes.panelTitle}>Affected resources</h2>
                <span className={classes.caption}>{selected.message}</span>
              </header>
              <div className={classes.refs}>
                {selected.resourceIds.slice(0, 60).map((id) => (
                  <ReferenceLink
                    key={id}
                    reference={`${run.type}/${id}`}
                    href={`${import.meta.env.BASE_URL}resources/${run.type}/${id}`}
                    onClick={(event) => {
                      event.preventDefault();
                      navigate(`/resources/${run.type}/${id}`);
                    }}
                  />
                ))}
                {selected.resourceIds.length > 60 && (
                  <span className={classes.caption}>+{selected.resourceIds.length - 60} more</span>
                )}
              </div>
            </section>
          )}
        </>
      )}
    </WorkspacePageLayout>
  );
}
