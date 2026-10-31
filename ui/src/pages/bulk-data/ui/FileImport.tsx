import { Button, Progress, StatusBadge } from "@octofhir/ui-kit";
import { useQueryClient } from "@tanstack/react-query";
import { FileUp } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { BATCH_SIZE, type ImportProgress, importFile } from "../lib/fileImport";
import classes from "./BulkDataPage.module.css";

function size(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/**
 * Imports a local NDJSON (or JSON Bundle) file from the browser in batch Bundles. Unlike
 * `$import`, which downloads from a URL the server can reach, this needs no file hosting.
 */
export function FileImport() {
  const queryClient = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const abort = useRef<AbortController | null>(null);
  const [file, setFile] = useState<File | null>(null);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => () => abort.current?.abort(), []);

  const start = async () => {
    if (!file) return;
    const controller = new AbortController();
    abort.current = controller;
    setRunning(true);
    setProgress({ read: 0, written: 0, failed: 0, errors: [] });
    try {
      await importFile(file, controller.signal, setProgress);
      void queryClient.invalidateQueries({ queryKey: ["fhir", "search"] });
    } finally {
      setRunning(false);
    }
  };

  // Bytes are the only total known up front; resources read stand in for progress.
  const done = progress ? progress.written + progress.failed : 0;

  return (
    <div className={classes.form}>
      <div className={classes.fields}>
        <input
          ref={input}
          type="file"
          accept=".ndjson,.jsonl,.json,application/fhir+ndjson,application/json"
          hidden
          onChange={(event) => {
            setFile(event.currentTarget.files?.[0] ?? null);
            setProgress(null);
          }}
        />
        <Button variant="outline" disabled={running} onClick={() => input.current?.click()}>
          <Button.Icon side="left">
            <FileUp size={14} />
          </Button.Icon>
          {file ? "Choose another file" : "Choose file"}
        </Button>
        {file && (
          <span className={classes.mono}>
            {file.name} · {size(file.size)}
          </span>
        )}
        {running ? (
          <Button className={classes.push} variant="outline" onClick={() => abort.current?.abort()}>
            Stop
          </Button>
        ) : (
          <Button className={classes.push} variant="filled" disabled={!file} onClick={() => void start()}>
            Import file
          </Button>
        )}
      </div>
      <span className={classes.hint}>
        NDJSON (one resource per line) or a JSON Bundle. Sent in batches of {BATCH_SIZE}; resources with an id keep it.
      </span>
      {progress && (
        <div className={classes.importResult}>
          {running && <Progress value={progress.read > 0 ? (done / progress.read) * 100 : 0} size="xs" />}
          <div className={classes.fields}>
            <StatusBadge tone={progress.failed > 0 ? "warning" : running ? "info" : "success"}>
              {running ? "Importing" : progress.failed > 0 ? "Finished with errors" : "Imported"}
            </StatusBadge>
            <span className={classes.muted}>
              {progress.written.toLocaleString()} written · {progress.failed.toLocaleString()} failed ·{" "}
              {progress.read.toLocaleString()} read
            </span>
          </div>
          {progress.errors.length > 0 && (
            <ul className={classes.importErrors}>
              {progress.errors.map((error) => (
                <li key={error} className={classes.error}>
                  {error}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
