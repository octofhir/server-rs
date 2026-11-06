import { Button, notify, TextArea, TextInput } from "@octofhir/ui-kit";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  closeSubmission,
  type SubmissionIdentity,
  type SubmissionStatusCode,
  startSubmissionStatus,
  submitManifest,
} from "../lib/api";
import classes from "./BulkDataPage.module.css";

const JOBS_KEY = ["bulk-data", "jobs"] as const;

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "The server rejected the request.";
}

/**
 * `$bulk-submit`: hand the server a manifest URL. The server fetches the manifest and its
 * files, so this needs the same reachability as `$import`. Submitters are configured on
 * the server; a submission stays open until it is completed or stopped.
 */
export function SubmitForm() {
  const queryClient = useQueryClient();
  const [submitterSystem, setSubmitterSystem] = useState("");
  const [submitterValue, setSubmitterValue] = useState("");
  const [submissionId, setSubmissionId] = useState("");
  const [manifestUrl, setManifestUrl] = useState("");
  const [fhirBaseUrl, setFhirBaseUrl] = useState("");
  const [replacesManifestUrl, setReplaces] = useState("");
  const [headers, setHeaders] = useState("");
  const [pending, setPending] = useState<string | null>(null);

  const identity: SubmissionIdentity = {
    submitterSystem: submitterSystem.trim(),
    submitterValue: submitterValue.trim(),
    submissionId: submissionId.trim(),
  };
  const identified = Boolean(identity.submitterSystem && identity.submitterValue && identity.submissionId);
  const ready = identified && /^https?:\/\//.test(manifestUrl.trim()) && /^https?:\/\//.test(fhirBaseUrl.trim());

  const run = async (action: string, work: () => Promise<unknown>) => {
    setPending(action);
    try {
      await work();
      await queryClient.invalidateQueries({ queryKey: JOBS_KEY });
    } catch (error) {
      notify({ theme: "danger", title: `${action} failed`, content: errorText(error) });
    } finally {
      setPending(null);
    }
  };

  const submit = () =>
    run("Submit", async () => {
      await submitManifest(identity, {
        manifestUrl: manifestUrl.trim(),
        fhirBaseUrl: fhirBaseUrl.trim(),
        replacesManifestUrl: replacesManifestUrl.trim() || undefined,
        fileHeaders: headers
          .split("\n")
          .map((line) => line.trim())
          .filter(Boolean),
      });
      setManifestUrl("");
      setReplaces("");
    });
  const close = (code: SubmissionStatusCode) =>
    run(code === "completed" ? "Complete" : "Stop", () => closeSubmission(identity, code));

  return (
    <div className={classes.form}>
      <div className={classes.fields}>
        <TextInput
          aria-label="Submitter system"
          placeholder="Submitter system"
          value={submitterSystem}
          onChange={setSubmitterSystem}
          className={classes.urlInput}
        />
        <TextInput
          aria-label="Submitter value"
          placeholder="Submitter value"
          value={submitterValue}
          onChange={setSubmitterValue}
          className={classes.groupInput}
        />
        <TextInput
          aria-label="Submission id"
          placeholder="Submission id"
          value={submissionId}
          onChange={setSubmissionId}
          className={classes.groupInput}
        />
      </div>
      <div className={classes.fields}>
        <TextInput
          aria-label="Manifest URL"
          placeholder="Manifest URL: https://example.org/manifest.json"
          value={manifestUrl}
          onChange={setManifestUrl}
          className={classes.urlInput}
        />
        <TextInput
          aria-label="FHIR base URL"
          placeholder="Sender's FHIR base URL"
          value={fhirBaseUrl}
          onChange={setFhirBaseUrl}
          className={classes.urlInput}
        />
      </div>
      <TextInput
        aria-label="Replaces manifest URL"
        placeholder="Replaces manifest URL (optional)"
        value={replacesManifestUrl}
        onChange={setReplaces}
      />
      <TextArea
        aria-label="File request headers"
        placeholder="File request headers, one per line: Authorization: Bearer …"
        rows={2}
        value={headers}
        onChange={setHeaders}
      />
      <div className={classes.fields}>
        <span className={classes.hint}>The server downloads the files itself, so the URLs must be reachable from it.</span>
        <div className={classes.push} />
        <Button
          variant="subtle"
          disabled={!identified}
          loading={pending === "Check status"}
          onClick={() => void run("Check status", () => startSubmissionStatus(identity))}
        >
          Check status
        </Button>
        <Button variant="outline" disabled={!identified} loading={pending === "Stop"} onClick={() => void close("stopped")}>
          Stop and take back
        </Button>
        <Button variant="outline" disabled={!identified} loading={pending === "Complete"} onClick={() => void close("completed")}>
          Complete
        </Button>
        <Button variant="filled" disabled={!ready} loading={pending === "Submit"} onClick={() => void submit()}>
          Submit manifest
        </Button>
      </div>
    </div>
  );
}
