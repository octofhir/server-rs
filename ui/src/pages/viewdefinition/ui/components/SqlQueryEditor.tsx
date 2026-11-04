import {
  type AutocompleteOption,
  Autocomplete,
  Badge,
  Button,
  Checkbox,
  IconPlus,
  IconTrash,
  Select,
  Tabs,
  Text,
  TextInput,
} from "@octofhir/ui-kit";
import { BookOpen, Braces, Database, GitBranch, Link2, ListTree, Network, Variable } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { setPgLspSettings } from "@/shared/monaco/lspClient";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import type { DependencyKind, DescribedDependency, Description } from "../../lib/sofApi";
import {
  invalidValue,
  libraryFromQuery,
  PARAMETER_TYPES,
  queryFromLibrary,
  type SqlDependency,
  type SqlParameter,
  type SqlQuery,
  sqlLibraryKind,
} from "../../lib/sqlQuery";
import classes from "./SqlQueryEditor.module.css";

const STATUS_OPTIONS = [
  { value: "draft", label: "Draft" },
  { value: "active", label: "Active" },
  { value: "retired", label: "Retired" },
  { value: "unknown", label: "Unknown" },
];

const KIND_OPTIONS = [
  { value: "sql-query", label: "SQLQuery" },
  { value: "sql-view", label: "SQLView" },
];

const TYPE_OPTIONS = PARAMETER_TYPES.map((t) => ({ value: t, label: t }));

/** The SQL language server in SQL on FHIR mode: dependencies, not the database. */
const SOF_LSP_PATH = "/api/lsp/pg?mode=sof";

const uid = () => crypto.randomUUID();

export const KIND_ICON: Record<DependencyKind, typeof Database> = {
  ViewDefinition: Database,
  SQLView: GitBranch,
  ValueSet: ListTree,
  ConceptMap: Network,
};

interface SqlQueryEditorProps {
  value: SqlQuery;
  onChange: (query: SqlQuery) => void;
  /** `$sql-describe` of the query, when it resolved. */
  description?: Description;
  /** Why the dependencies did not resolve. */
  describeError?: string;
  /** Canonicals to suggest for a dependency. */
  suggestions: AutocompleteOption[];
  onRun: () => void;
}

export function SqlQueryEditor({
  value: query,
  onChange,
  description,
  describeError,
  suggestions,
  onRun,
}: SqlQueryEditorProps) {
  // The editor binds ⌘↵ once at mount; it calls whatever run is current.
  const runRef = useRef(onRun);
  runRef.current = onRun;
  const execute = useCallback(() => runRef.current(), []);

  // The language server knows the query's relations (with the columns the
  // server resolved) and its parameters.
  useEffect(() => {
    const described = new Map(description?.dependencies.map((d) => [d.label, d]) ?? []);
    setPgLspSettings({
      sof: {
        relations: query.dependencies
          .filter((d) => d.label.trim())
          .map((d) => {
            const known = described.get(d.label.trim().toLowerCase());
            return {
              name: d.label.trim(),
              kind: known?.kind,
              columns: known?.columns ?? [],
            };
          }),
        parameters:
          query.kind === "sql-query"
            ? query.parameters.filter((p) => p.name.trim()).map((p) => ({ name: p.name.trim(), type: p.type }))
            : [],
      },
    });
  }, [description, query.dependencies, query.parameters, query.kind]);

  const jsonText = useMemo(() => JSON.stringify(libraryFromQuery(query), null, 2), [query]);
  const handleJsonChange = useCallback(
    (text: string) => {
      try {
        const parsed: unknown = JSON.parse(text);
        if (parsed && typeof parsed === "object" && sqlLibraryKind(parsed)) {
          const next = queryFromLibrary(parsed as Record<string, unknown>);
          next.parameters = next.parameters.map((p) => ({
            ...p,
            value: query.parameters.find((q) => q.name === p.name)?.value,
          }));
          onChange(next);
        }
      } catch {
        // ignore invalid JSON while typing
      }
    },
    [onChange, query.parameters]
  );

  const set = <K extends keyof SqlQuery>(key: K, val: SqlQuery[K]) => onChange({ ...query, [key]: val });
  const described = new Map(description?.dependencies.map((d) => [d.label, d]) ?? []);

  return (
    <div className={classes.panel}>
      <Tabs defaultValue="sql" className={classes.tabs}>
        <Tabs.List>
          <Tabs.Tab value="sql">
            <Database size={13} /> SQL
          </Tabs.Tab>
          <Tabs.Tab value="json">
            <Braces size={13} /> JSON
          </Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="sql" className={classes.tabPanel}>
          <div className={classes.layout}>
            <div className={classes.meta}>
              <TextInput
                label="Name"
                placeholder="cvd_patients"
                value={query.name}
                onChange={(v) => set("name", v)}
                className={classes.metaName}
                size="xs"
              />
              <Select
                label="Profile"
                value={query.kind}
                onUpdate={(v) => set("kind", v === "sql-view" ? "sql-view" : "sql-query")}
                data={KIND_OPTIONS}
                size="xs"
                className={classes.metaSmall}
              />
              <Select
                label="Status"
                value={query.status}
                onUpdate={(v) =>
                  set(
                    "status",
                    v === "active" || v === "retired" || v === "unknown" ? v : "draft"
                  )
                }
                data={STATUS_OPTIONS}
                size="xs"
                className={classes.metaSmall}
              />
              <TextInput
                label="URL"
                placeholder="https://example.org/Library/cvd-patients"
                value={query.url ?? ""}
                onChange={(v) => set("url", v || undefined)}
                className={classes.metaUrl}
                size="xs"
              />
            </div>

            <div className={classes.editor}>
              <SqlEditor
                value={query.sql}
                onChange={(sql) => set("sql", sql)}
                onExecute={execute}
                lspPath={SOF_LSP_PATH}
              />
            </div>

            <div className={classes.sections}>
              <DependenciesSection
                dependencies={query.dependencies}
                described={described}
                describeError={describeError}
                check={description?.check}
                suggestions={suggestions}
                onChange={(dependencies) => set("dependencies", dependencies)}
              />
              {query.kind === "sql-query" ? (
                <ParametersSection
                  parameters={query.parameters}
                  onChange={(parameters) => set("parameters", parameters)}
                />
              ) : (
                <Text size="xs" c="dimmed" className={classes.note}>
                  A SQLView declares no parameters; other queries read it as a dependency.
                </Text>
              )}
            </div>
          </div>
        </Tabs.Panel>

        <Tabs.Panel value="json" className={classes.tabPanel}>
          <JsonEditor value={jsonText} onChange={handleJsonChange} height="100%" />
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}

function SectionHead({
  icon,
  title,
  count,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  count: number;
  action: React.ReactNode;
}) {
  return (
    <header className={classes.sectionHead}>
      <span className={classes.sectionTitle}>
        {icon}
        <Text size="sm" fw={600}>
          {title}
        </Text>
        {count > 0 && (
          <Badge size="xs" variant="light">
            {count}
          </Badge>
        )}
      </span>
      {action}
    </header>
  );
}

function DependenciesSection({
  dependencies,
  described,
  describeError,
  check,
  suggestions,
  onChange,
}: {
  dependencies: SqlDependency[];
  described: Map<string, DescribedDependency>;
  describeError?: string;
  check?: string;
  suggestions: AutocompleteOption[];
  onChange: (dependencies: SqlDependency[]) => void;
}) {
  const update = (index: number, patch: Partial<SqlDependency>) =>
    onChange(dependencies.map((d, i) => (i === index ? { ...d, ...patch } : d)));

  return (
    <section className={classes.section}>
      <SectionHead
        icon={<BookOpen size={14} />}
        title="Dependencies"
        count={dependencies.length}
        action={
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconPlus size={12} />}
            onClick={() => onChange([...dependencies, { label: "", canonical: "", _id: uid() }])}
          >
            Dependency
          </Button>
        }
      />
      {dependencies.length === 0 ? (
        <Text size="xs" c="dimmed" className={classes.note}>
          Each table the SQL reads is a dependency: a ViewDefinition, a SQLView, a ValueSet or a
          ConceptMap, named in the SQL by its label.
        </Text>
      ) : (
        <div className={classes.depGrid}>
          <span className={classes.colHead}>Label</span>
          <span className={classes.colHead}>Canonical</span>
          <span className={classes.colHead}>Resolved</span>
          <span />
          {dependencies.map((dep, index) => {
            const info = described.get(dep.label.trim().toLowerCase());
            return (
              <DependencyRow
                key={dep._id ?? index}
                dependency={dep}
                info={info}
                suggestions={suggestions}
                onChange={(patch) => update(index, patch)}
                onRemove={() => onChange(dependencies.filter((_, i) => i !== index))}
              />
            );
          })}
        </div>
      )}
      {describeError && (
        <Text size="xs" className={classes.error}>
          {describeError}
        </Text>
      )}
      {!describeError && check && (
        <Text size="xs" className={classes.error}>
          {check}
        </Text>
      )}
    </section>
  );
}

function DependencyRow({
  dependency,
  info,
  suggestions,
  onChange,
  onRemove,
}: {
  dependency: SqlDependency;
  info?: DescribedDependency;
  suggestions: AutocompleteOption[];
  onChange: (patch: Partial<SqlDependency>) => void;
  onRemove: () => void;
}) {
  const Icon = info ? KIND_ICON[info.kind] : undefined;
  return (
    <>
      <TextInput
        size="xs"
        value={dependency.label}
        onChange={(label) => onChange({ label })}
        placeholder="patient"
        className={classes.mono}
        aria-label="Dependency label"
      />
      <Autocomplete
        size="xs"
        value={dependency.canonical}
        onChange={(canonical) => onChange({ canonical })}
        data={suggestions}
        placeholder="https://example.org/ViewDefinition/patient|1.0"
        leftSection={<Link2 size={13} />}
        aria-label="Dependency canonical"
      />
      <span className={classes.resolved}>
        {info && Icon ? (
          <>
            <Badge size="xs" variant="light" leftSection={<Icon size={11} />}>
              {info.kind}
            </Badge>
            <span className={classes.resolvedText} title={info.resolved}>
              {resolvedSummary(info)}
            </span>
          </>
        ) : (
          <span className={classes.pending}>—</span>
        )}
      </span>
      <Button variant="subtle" size="xs" aria-label="Remove dependency" onClick={onRemove}>
        <IconTrash size={13} />
      </Button>
    </>
  );
}

/** Version, content size and source of a resolved dependency, in a few words. */
export function resolvedSummary(info: DescribedDependency): string {
  const version = info.resolved?.includes("|") ? info.resolved.split("|").pop() : undefined;
  const bits: string[] = [];
  if (version) bits.push(`v${version}`);
  const p = info.provenance;
  if (p?.members !== undefined) bits.push(`${p.members} codes`);
  if (p?.mappings !== undefined) bits.push(`${p.mappings} mappings`);
  if (p?.source) bits.push(p.source);
  if (bits.length === 0) bits.push(`${info.columns.length} cols`);
  return bits.join(" · ");
}

function ParametersSection({
  parameters,
  onChange,
}: {
  parameters: SqlParameter[];
  onChange: (parameters: SqlParameter[]) => void;
}) {
  const update = (index: number, patch: Partial<SqlParameter>) =>
    onChange(parameters.map((p, i) => (i === index ? { ...p, ...patch } : p)));

  return (
    <section className={classes.section}>
      <SectionHead
        icon={<Variable size={14} />}
        title="Parameters"
        count={parameters.length}
        action={
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconPlus size={12} />}
            onClick={() =>
              onChange([...parameters, { name: "", type: "string", required: false, _id: uid() }])
            }
          >
            Parameter
          </Button>
        }
      />
      {parameters.length === 0 ? (
        <Text size="xs" c="dimmed" className={classes.note}>
          Write <code className={classes.inlineCode}>:name</code> in the SQL and declare it here;
          values are bound, never interpolated.
        </Text>
      ) : (
        <div className={classes.paramGrid}>
          <span className={classes.colHead}>Name</span>
          <span className={classes.colHead}>Type</span>
          <span className={classes.colHead}>Required</span>
          <span className={classes.colHead}>Value for this run</span>
          <span />
          {parameters.map((param, index) => (
            <ParameterRow
              key={param._id ?? index}
              parameter={param}
              onChange={(patch) => update(index, patch)}
              onRemove={() => onChange(parameters.filter((_, i) => i !== index))}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function ParameterRow({
  parameter,
  onChange,
  onRemove,
}: {
  parameter: SqlParameter;
  onChange: (patch: Partial<SqlParameter>) => void;
  onRemove: () => void;
}) {
  return (
    <>
      <TextInput
        size="xs"
        value={parameter.name}
        onChange={(name) => onChange({ name })}
        placeholder="from_date"
        className={classes.mono}
        aria-label="Parameter name"
      />
      <Select
        size="xs"
        value={parameter.type}
        onUpdate={(type) => onChange({ type: type ?? "string" })}
        data={TYPE_OPTIONS}
        searchable
        aria-label="Parameter type"
      />
      <span className={classes.center}>
        <Checkbox
          checked={parameter.required}
          onChange={(required) => onChange({ required })}
          aria-label="Required"
        />
      </span>
      <TextInput
        size="xs"
        value={parameter.value ?? ""}
        onChange={(value) => onChange({ value })}
        placeholder={placeholderFor(parameter.type)}
        error={invalidValue(parameter) ? `Not a ${parameter.type}` : undefined}
        className={classes.mono}
        aria-label="Parameter value"
      />
      <Button variant="subtle" size="xs" aria-label="Remove parameter" onClick={onRemove}>
        <IconTrash size={13} />
      </Button>
    </>
  );
}

function placeholderFor(type: string): string {
  switch (type) {
    case "date":
      return "2024-01-01";
    case "dateTime":
    case "instant":
      return "2024-01-01T00:00:00Z";
    case "boolean":
      return "true";
    case "integer":
    case "positiveInt":
    case "unsignedInt":
    case "integer64":
      return "10";
    case "decimal":
      return "1.5";
    default:
      return "unbound (null)";
  }
}
