import { Button, TextInput } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { type CanonicalHit, type Parameter, parameterValue, translate } from "../lib/api";
import { OutcomeNote, ResultLine } from "./OutcomeNote";
import classes from "./Terminology.module.css";

interface Group {
  source?: string;
  target?: string;
  element?: unknown[];
}

interface Match {
  relationship?: string;
  system?: string;
  code?: string;
  display?: string;
}

/** `match` output parameters of $translate (R4 `equivalence`, R5 `relationship`). */
function matches(parameters: Parameter[] | undefined): Match[] {
  return (parameters ?? [])
    .filter((parameter) => parameter.name === "match")
    .map((match) => {
      const part = (name: string) => match.part?.find((entry) => entry.name === name);
      const concept = part("concept")?.valueCoding;
      return {
        relationship: part("relationship")?.valueCode ?? part("equivalence")?.valueCode,
        system: concept?.system,
        code: concept?.code,
        display: concept?.display,
      };
    });
}

export function ConceptMapPanel({ hit, resource }: { hit: CanonicalHit; resource?: Record<string, unknown> }) {
  const groups = (resource?.group as Group[] | undefined) ?? [];
  const [system, setSystem] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const run = useMutation({ mutationFn: () => translate(hit.url, system ?? groups[0]?.source ?? "", code) });
  const found = matches(run.data?.parameter);

  return (
    <>
      <section className={classes.block} aria-label="Groups">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Groups</h3>
          <span className={classes.muted}>{groups.length}</span>
        </header>
        {groups.length === 0 ? (
          <p className={classes.muted}>This concept map has no groups.</p>
        ) : (
          <ul className={classes.groups}>
            {groups.map((group, index) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: groups have no id; order is stable
              <li key={index} className={classes.group}>
                <span className={classes.system}>{group.source ?? "—"}</span>
                <span className={classes.muted}>→</span>
                <span className={classes.system}>{group.target ?? "—"}</span>
                <span className={classes.groupCount}>{(group.element?.length ?? 0).toLocaleString()} codes</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={classes.block} aria-label="Translate">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Translate</h3>
          <span className={classes.muted}>$translate</span>
        </header>
        <form
          className={classes.row}
          onSubmit={(event) => {
            event.preventDefault();
            if (code.trim()) run.mutate();
          }}
        >
          <TextInput
            className={classes.systemInput}
            aria-label="Source system"
            placeholder="Source system URL"
            value={system ?? groups[0]?.source ?? ""}
            onChange={setSystem}
          />
          <TextInput className={classes.codeInput} aria-label="Code" placeholder="Code" value={code} onChange={setCode} />
          <Button type="submit" variant="outline" loading={run.isPending} disabled={!code.trim()}>
            Translate
          </Button>
        </form>
        {run.isError && <OutcomeNote error={run.error} />}
        {run.data &&
          (found.length === 0 ? (
            <ResultLine
              ok={false}
              label="No match"
              detail={String(parameterValue(run.data, "message") ?? "")}
            />
          ) : (
            <ul className={classes.groups}>
              {found.map((match) => (
                <li key={`${match.system}|${match.code}|${match.relationship}`} className={classes.group}>
                  <span className={classes.code}>{match.code}</span>
                  <span>{match.display}</span>
                  <span className={classes.system}>{match.system}</span>
                  <span className={classes.groupCount}>{match.relationship}</span>
                </li>
              ))}
            </ul>
          ))}
      </section>
    </>
  );
}
