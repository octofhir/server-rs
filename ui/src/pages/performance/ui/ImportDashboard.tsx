import { Alert, Button, Modal, TextArea, TextInput } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { FileUp } from "lucide-react";
import { type ChangeEvent, useState } from "react";
import { type Dashboard, type ImportedDashboard, metricsApi, useSaveDashboard } from "@/entities/metrics";
import classes from "./PerformancePage.module.css";

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "Import failed";
}

/**
 * Imports a Grafana dashboard JSON (or one downloaded from this console): the server converts it
 * and lists what it could not carry over, then it is saved as a new dashboard.
 */
export function ImportDashboard({ onClose, onCreated }: { onClose: () => void; onCreated: (dashboard: Dashboard) => void }) {
  const [text, setText] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const [preview, setPreview] = useState<ImportedDashboard | null>(null);
  const [title, setTitle] = useState("");
  const save = useSaveDashboard();

  const convert = useMutation({
    mutationFn: (json: unknown) => metricsApi.importDashboard(json),
    onSuccess: (result) => {
      setPreview(result);
      setTitle(result.dashboard.title);
    },
  });

  const submitText = (value: string) => {
    setPreview(null);
    try {
      const json = JSON.parse(value);
      setParseError(null);
      convert.mutate(json);
    } catch {
      setParseError("This is not valid JSON.");
    }
  };

  const pickFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    const content = await file.text();
    setText(content);
    submitText(content);
  };

  const panels = preview?.dashboard.panels ?? [];
  const sections = panels.filter((p) => p.type === "row").length;

  return (
    <Modal
      open
      onClose={onClose}
      title="Import dashboard"
      size="lg"
      footer={
        <>
          <Button variant="subtle" onClick={onClose}>
            Cancel
          </Button>
          {preview ? (
            <Button
              color="primary"
              loading={save.isPending}
              disabled={!title.trim()}
              onClick={async () => {
                const created = await save.mutateAsync({ ...preview.dashboard, title: title.trim() });
                onCreated(created);
              }}
            >
              Create dashboard
            </Button>
          ) : (
            <Button color="primary" loading={convert.isPending} disabled={!text.trim()} onClick={() => submitText(text)}>
              Convert
            </Button>
          )}
        </>
      }
    >
      <div className={classes.importBody}>
        <p className={classes.exportText}>
          A Grafana dashboard JSON (<i>Share → Export</i>, or the API's <code>{"{ dashboard: … }"}</code>) or a dashboard
          downloaded from this console. PromQL panels, sections, units, thresholds, colours and{" "}
          <code>label_values</code> variables carry over; queries are scoped to this deployment when they run.
        </p>
        <label className={classes.fileButton}>
          <FileUp size={14} aria-hidden="true" />
          Choose a file…
          <input type="file" accept="application/json,.json" onChange={pickFile} />
        </label>
        <TextArea
          aria-label="Dashboard JSON"
          value={text}
          onChange={(value) => {
            setText(value);
            setPreview(null);
          }}
          placeholder='{"title": "…", "panels": [ … ]}'
          minRows={8}
        />
        {parseError ? <Alert theme="danger" message={parseError} /> : null}
        {convert.error ? <Alert theme="danger" message={errorText(convert.error)} /> : null}
        {preview ? (
          <div className={classes.importPreview}>
            <TextInput label="Title" value={title} onChange={setTitle} w="100%" />
            <p className={classes.exportText}>
              From {preview.source === "grafana" ? "Grafana" : "OctoFHIR"}: {plural(panels.length - sections, "panel")}
              {sections ? ` in ${plural(sections, "section")}` : ""}, {plural(preview.dashboard.variables?.length ?? 0, "variable")}.
            </p>
            {preview.warnings.length > 0 ? (
              <Alert
                theme="warning"
                title={`${preview.warnings.length} item${preview.warnings.length === 1 ? "" : "s"} not imported`}
                message={
                  <ul className={classes.warnings}>
                    {preview.warnings.map((warning) => (
                      <li key={warning}>{warning}</li>
                    ))}
                  </ul>
                }
              />
            ) : null}
            {save.error ? <Alert theme="danger" message={errorText(save.error)} /> : null}
          </div>
        ) : null}
      </div>
    </Modal>
  );
}
