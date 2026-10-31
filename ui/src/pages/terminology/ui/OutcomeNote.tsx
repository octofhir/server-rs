import { OperationOutcomePanel } from "@octofhir/ui-kit";
import { Check, X } from "lucide-react";
import { TerminologyError } from "../lib/api";
import classes from "./Terminology.module.css";

/** A failed operation: the server's OperationOutcome when it sent one, else the message. */
export function OutcomeNote({ error }: { error: unknown }) {
  if (error instanceof TerminologyError && error.outcome) {
    return <OperationOutcomePanel outcome={error.outcome} maxIssues={3} />;
  }
  return <p className={classes.error}>{error instanceof Error ? error.message : "The request failed."}</p>;
}

/** Yes/no answer of an operation, with an icon so it is not told by colour alone. */
export function ResultLine({ ok, label, detail }: { ok: boolean; label: string; detail?: string }) {
  return (
    <p className={classes.result} data-ok={ok}>
      {ok ? <Check size={14} aria-hidden="true" /> : <X size={14} aria-hidden="true" />}
      <span className={classes.resultLabel}>{label}</span>
      {detail ? <span className={classes.muted}>{detail}</span> : null}
    </p>
  );
}
