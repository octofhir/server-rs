import { Select, TextArea, TextInput } from "@octofhir/ui-kit";
import { CqlSourceEditor } from "@/pages/cql-console/components/CqlSourceEditor";
import { FhirPathEditor } from "@/shared/monaco/FhirPathEditor";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import type { Cell, HttpMethod, Scope, Variable } from "../model/notebook";
import classes from "../NotebookEditor.module.css";
import { ChartCellEditor } from "./ChartCellEditor";
import { InputCellEditor } from "./InputCellEditor";
import { MarkdownCellEditor } from "./MarkdownCellEditor";
import { PipelineCellEditor } from "./PipelineCellEditor";

const HTTP_METHODS: HttpMethod[] = ["GET", "POST", "PUT", "DELETE", "PATCH"];

interface Props {
  cell: Cell;
  onChange: (next: Cell) => void;
  onRun: () => void;
  namedCells: { id: string; name: string; label: string }[];
  variables: Variable[];
  scope: Scope;
}

export function CellEditor({ cell, onChange, onRun, namedCells, variables, scope }: Props) {
  switch (cell.type) {
    case "markdown":
      return (
        <MarkdownCellEditor
          value={cell.source}
          onChange={(v) => onChange({ ...cell, source: v })}
        />
      );

    case "fhirpath":
      return (
        <div className={classes.fhirpathEditor}>
          <div className={classes.editorHost}>
            <FhirPathEditor
              value={cell.source}
              onChange={(v) => onChange({ ...cell, source: v })}
              onSubmit={onRun}
              height="100%"
              placeholder="e.g.  Patient.name.given   or   1 + 1"
            />
          </div>
          <div className={classes.fhirpathCtx}>
            <span className={classes.ctxLabel}>Context</span>
            <TextInput
              value={cell.config?.contextRef ?? ""}
              onChange={(ref) =>
                onChange({
                  ...cell,
                  config: { ...cell.config, contextRef: ref || undefined },
                })
              }
              placeholder="optional — Patient/123 (leave empty to run a bare expression)"
              size="sm"
              className={classes.ctxInput}
            />
          </div>
        </div>
      );

    case "sql":
      return (
        <div className={classes.editorHostTall}>
          <SqlEditor
            value={cell.source}
            onChange={(v) => onChange({ ...cell, source: v })}
            onExecute={onRun}
            height="100%"
          />
        </div>
      );

    case "sql-on-fhir":
      return (
        <div className={classes.editorHostTall}>
          <JsonEditor
            value={JSON.stringify(cell.source, null, 2)}
            onChange={(v) => {
              try {
                onChange({ ...cell, source: JSON.parse(v) });
              } catch {
                /* keep last valid until parseable */
              }
            }}
            onExecute={onRun}
            height="100%"
          />
        </div>
      );

    case "cql":
      return (
        <div className={classes.editorHostTall}>
          <CqlSourceEditor
            value={cell.source}
            onChange={(v) => onChange({ ...cell, source: v })}
            onSubmit={onRun}
            height="100%"
          />
        </div>
      );

    case "graphql":
      return (
        <TextArea
          value={cell.source}
          onChange={(v) => onChange({ ...cell, source: v })}
          rows={8}
          className={classes.codeArea}
          placeholder="GraphQL query"
        />
      );

    case "rest":
      return (
        <div className={classes.restEditor}>
          <div className={classes.restBar}>
            <Select
              data={HTTP_METHODS.map((m) => ({ value: m, label: m }))}
              value={cell.source.method}
              onChange={(m) =>
                m && onChange({ ...cell, source: { ...cell.source, method: m as HttpMethod } })
              }
              size="sm"
              className={classes.restMethod}
            />
            <TextInput
              value={cell.source.url}
              onChange={(url) => onChange({ ...cell, source: { ...cell.source, url } })}
              placeholder="/Patient?_count=10"
              size="sm"
              className={classes.restUrl}
            />
          </div>
          {cell.source.method !== "GET" && (
            <TextArea
              value={
                typeof cell.source.body === "string"
                  ? cell.source.body
                  : cell.source.body
                    ? JSON.stringify(cell.source.body, null, 2)
                    : ""
              }
              onChange={(body) => onChange({ ...cell, source: { ...cell.source, body } })}
              rows={6}
              className={classes.codeArea}
              placeholder="Request body (JSON)"
            />
          )}
        </div>
      );

    case "input":
      return <InputCellEditor cell={cell} onChange={onChange} variables={variables} />;

    case "chart":
      return (
        <ChartCellEditor cell={cell} onChange={onChange} namedCells={namedCells} scope={scope} />
      );

    case "pipeline":
      return (
        <PipelineCellEditor cell={cell} onChange={onChange} namedCells={namedCells} scope={scope} />
      );

    default:
      return null;
  }
}
