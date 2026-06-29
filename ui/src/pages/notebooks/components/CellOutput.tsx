import { Chart, DataTable, MarkdownView } from "@octofhir/ui-kit";
import { CircleAlert } from "lucide-react";
import { useMemo } from "react";
import type { Output, Scope } from "../model/notebook";
import classes from "../NotebookEditor.module.css";

function TableOutput({ columns, rows, meta }: Extract<Output, { kind: "table" }>) {
  const cols = useMemo(
    () =>
      columns.map((c) => ({
        id: c,
        header: c,
        accessor: (row: unknown[]) => row[columns.indexOf(c)],
        cell: (row: unknown[]) => {
          const v = row[columns.indexOf(c)];
          if (v === null || v === undefined) return <span className={classes.nullCell}>∅</span>;
          return typeof v === "object" ? JSON.stringify(v) : String(v);
        },
      })),
    [columns]
  );
  return (
    <div>
      <div className={classes.outMeta}>
        {meta.rowCount} rows
        {meta.executionTimeMs != null && ` · ${meta.executionTimeMs}ms`}
        {meta.truncated && " · truncated"}
      </div>
      <DataTable data={rows} columns={cols} paginated pageSize={25} />
    </div>
  );
}

export function CellOutput({ output, scope }: { output: Output; scope: Scope }) {
  switch (output.kind) {
    case "markdown":
      return <MarkdownView source={output.text} scope={scope} />;

    case "table":
      return <TableOutput {...output} />;

    case "value":
      return (
        <div className={classes.valueList}>
          {output.meta?.totalTime != null && (
            <div className={classes.outMeta}>
              {output.data.length} result(s) · {output.meta.totalTime.toFixed(2)}ms
            </div>
          )}
          {output.data.length === 0 ? (
            <div className={classes.emptyHint}>Empty collection</div>
          ) : (
            output.data.map((v, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: positional results
              <div key={i} className={classes.valueItem}>
                <span className={classes.valueIdx}>{i}</span>
                <code>{typeof v === "object" ? JSON.stringify(v) : String(v)}</code>
              </div>
            ))
          )}
        </div>
      );

    case "json":
    case "bundle":
      return (
        <pre className={classes.jsonOut}>
          {JSON.stringify(output.kind === "json" ? output.data : output.data, null, 2)}
        </pre>
      );

    case "chart":
      return <Chart option={(output.spec as Record<string, unknown>) ?? {}} height={320} />;

    case "html":
      // biome-ignore lint/security/noDangerouslySetInnerHtml: sanitized upstream
      return <div dangerouslySetInnerHTML={{ __html: output.html }} />;

    case "error":
      return (
        <div className={classes.errorBox}>
          <CircleAlert size={16} className={classes.errorIcon} />
          <span className={classes.errorMsg}>{output.message}</span>
        </div>
      );

    default:
      return null;
  }
}
