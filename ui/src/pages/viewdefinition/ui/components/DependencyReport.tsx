import { Badge, Text } from "@octofhir/ui-kit";
import type { Description, TerminologyRecord } from "../../lib/sofApi";
import classes from "./DependencyReport.module.css";
import { KIND_ICON } from "./SqlQueryEditor";

interface DependencyReportProps {
  description?: Description;
  describeError?: string;
  /** Terminology the last run resolved. */
  terminology: TerminologyRecord[];
}

/**
 * How a SQL query's dependencies resolve: each label's artifact, the version
 * it resolved to, its columns, and for a ValueSet or ConceptMap where its
 * content came from and how large it is (Terminology in SQL: one resolution
 * per job, before any SQL runs).
 */
export function DependencyReport({ description, describeError, terminology }: DependencyReportProps) {
  if (describeError) {
    return (
      <div className={classes.root}>
        <Text size="xs" className={classes.error}>
          {describeError}
        </Text>
      </div>
    );
  }
  if (!description) {
    return (
      <div className={classes.root}>
        <Text size="sm" c="dimmed">
          Resolving dependencies…
        </Text>
      </div>
    );
  }
  return (
    <div className={classes.root}>
      {description.check && (
        <div className={classes.block}>
          <Text size="xs" fw={600}>
            The SQL does not prepare
          </Text>
          <Text size="xs" className={classes.error}>
            {description.check}
          </Text>
        </div>
      )}

      {description.columns.length > 0 && (
        <div className={classes.block}>
          <Text size="xs" fw={600}>
            Result
          </Text>
          <ColumnList columns={description.columns} />
        </div>
      )}

      {description.dependencies.map((dep) => {
        const Icon = KIND_ICON[dep.kind];
        const p = dep.provenance;
        return (
          <div key={dep.label} className={classes.block}>
            <div className={classes.head}>
              <code className={classes.label}>{dep.label}</code>
              <Badge size="xs" variant="light" leftSection={<Icon size={11} />}>
                {dep.kind}
              </Badge>
            </div>
            <code className={classes.canonical}>{dep.resolved ?? dep.canonical}</code>
            {p && (
              <dl className={classes.facts}>
                {p.members !== undefined && <Fact term="Codes" value={String(p.members)} />}
                {p.mappings !== undefined && <Fact term="Mappings" value={String(p.mappings)} />}
                {p.source && <Fact term="Source" value={p.source} />}
                {p.expandedBy && <Fact term="Expanded by" value={p.expandedBy} />}
              </dl>
            )}
            <ColumnList columns={dep.columns} />
          </div>
        );
      })}

      {description.dependencies.length === 0 && (
        <Text size="sm" c="dimmed">
          The query declares no dependencies.
        </Text>
      )}

      {terminology.length > 0 && (
        <div className={classes.block}>
          <Text size="xs" fw={600}>
            Last run
          </Text>
          {terminology.map((t) => (
            <code key={`${t.url}|${t.version ?? ""}`} className={classes.canonical}>
              {t.resourceType} {t.url}
              {t.version ? `|${t.version}` : ""} ·{" "}
              {t.members !== undefined ? `${t.members} codes` : `${t.mappings ?? 0} mappings`}
              {t.source ? ` · ${t.source}` : ""}
            </code>
          ))}
        </div>
      )}
    </div>
  );
}

function Fact({ term, value }: { term: string; value: string }) {
  return (
    <>
      <dt>{term}</dt>
      <dd>{value}</dd>
    </>
  );
}

function ColumnList({ columns }: { columns: Description["columns"] }) {
  if (columns.length === 0) return null;
  return (
    <ul className={classes.columns}>
      {columns.map((c) => (
        <li key={c.name}>
          <code>{c.name}</code>
          <span className={classes.type}>{c.type}</span>
        </li>
      ))}
    </ul>
  );
}
