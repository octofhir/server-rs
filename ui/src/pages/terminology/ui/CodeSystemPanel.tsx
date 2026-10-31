import { Button, TextInput, useDebouncedValue } from "@octofhir/ui-kit";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { type CanonicalHit, lookupCode, type Parameter, parameterValue, subsumes } from "../lib/api";
import { OutcomeNote, ResultLine } from "./OutcomeNote";
import classes from "./Terminology.module.css";

/** Rows rendered at once; a filter narrows larger systems. */
const MAX_ROWS = 500;

interface Concept {
  code?: string;
  display?: string;
  concept?: Concept[];
}

interface ConceptRow {
  code: string;
  display?: string;
  depth: number;
}

function flatten(concepts: Concept[] | undefined, depth = 0, out: ConceptRow[] = []): ConceptRow[] {
  for (const concept of concepts ?? []) {
    if (concept.code) out.push({ code: concept.code, display: concept.display, depth });
    flatten(concept.concept, depth + 1, out);
  }
  return out;
}

const SUBSUMPTION_LABELS: Record<string, string> = {
  equivalent: "A and B are equivalent",
  subsumes: "A subsumes B",
  "subsumed-by": "A is subsumed by B",
  "not-subsumed": "Neither subsumes the other",
};

/** Lookup output worth showing: properties and designations, flattened to name/value pairs. */
function lookupRows(parameters: Parameter[] | undefined): [string, string][] {
  return (parameters ?? []).flatMap((parameter): [string, string][] => {
    if (parameter.part) {
      const label = parameter.part.find((part) => part.name === "code" || part.name === "language");
      const value = parameter.part.find((part) => part.name === "value" || part.name === "valueString");
      const text =
        value?.valueString ?? value?.valueCode ?? value?.valueCoding?.code ?? String(value?.valueBoolean ?? "");
      return [[`${parameter.name} · ${label?.valueCode ?? label?.valueString ?? ""}`, text]];
    }
    const text = parameter.valueString ?? parameter.valueCode ?? parameter.valueUri ?? parameter.valueBoolean;
    return text == null ? [] : [[parameter.name, String(text)]];
  });
}

export function CodeSystemPanel({ hit, resource }: { hit: CanonicalHit; resource?: Record<string, unknown> }) {
  const rows = useMemo(() => flatten(resource?.concept as Concept[] | undefined), [resource]);
  const [filter, setFilter] = useState("");
  const debounced = useDebouncedValue(filter, 150).trim().toLowerCase();
  const matches = useMemo(
    () =>
      debounced
        ? rows.filter(
            (row) => row.code.toLowerCase().includes(debounced) || row.display?.toLowerCase().includes(debounced)
          )
        : rows,
    [rows, debounced]
  );
  const [selected, setSelected] = useState<string | null>(null);
  const lookup = useQuery({
    queryKey: ["terminology", "lookup", hit.url, selected],
    queryFn: () => lookupCode(hit.url, selected ?? ""),
    enabled: selected != null,
    retry: false,
  });

  const [codeA, setCodeA] = useState("");
  const [codeB, setCodeB] = useState("");
  const check = useMutation({ mutationFn: () => subsumes(hit.url, codeA, codeB) });
  const content = typeof resource?.content === "string" ? resource.content : undefined;

  return (
    <>
      <section className={classes.block} aria-label="Concepts">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Concepts</h3>
          <span className={classes.muted}>
            {debounced ? `${matches.length.toLocaleString()} of ` : ""}
            {rows.length.toLocaleString()} codes
          </span>
          <TextInput
            className={classes.blockFilter}
            aria-label="Filter concepts"
            placeholder="Filter concepts…"
            value={filter}
            onChange={setFilter}
          />
        </header>
        {rows.length === 0 ? (
          <p className={classes.muted}>
            {content && content !== "complete"
              ? `The package carries no concepts for this code system (content: ${content}).`
              : "This code system defines no concepts."}
          </p>
        ) : (
          <div className={classes.split}>
            <ul className={classes.concepts}>
              {matches.slice(0, MAX_ROWS).map((row) => (
                <li key={row.code}>
                  <button
                    type="button"
                    className={classes.concept}
                    aria-pressed={row.code === selected}
                    style={{ paddingLeft: 8 + (debounced ? 0 : row.depth * 14) }}
                    onClick={() => setSelected(row.code)}
                  >
                    <span className={classes.code}>{row.code}</span>
                    <span className={classes.conceptDisplay}>{row.display}</span>
                  </button>
                </li>
              ))}
              {matches.length > MAX_ROWS && (
                <li className={classes.muted}>
                  {(matches.length - MAX_ROWS).toLocaleString()} more; filter to narrow the list.
                </li>
              )}
            </ul>
            <div className={classes.lookup} aria-live="polite">
              {selected == null ? (
                <p className={classes.muted}>Pick a concept to see its $lookup properties.</p>
              ) : lookup.isError ? (
                <OutcomeNote error={lookup.error} />
              ) : (
                <>
                  <p className={classes.lookupTitle}>
                    <span className={classes.code}>{selected}</span>
                    {String(parameterValue(lookup.data, "display") ?? "")}
                  </p>
                  <dl className={classes.properties}>
                    {lookupRows(lookup.data?.parameter)
                      // display is the title; name and version describe the code system, not the concept.
                      .filter(([name]) => !["display", "name", "version"].includes(name))
                      .map(([name, value]) => (
                        <div key={`${name}=${value}`} className={classes.property}>
                          <dt>{name}</dt>
                          <dd>{value}</dd>
                        </div>
                      ))}
                  </dl>
                </>
              )}
            </div>
          </div>
        )}
      </section>

      <section className={classes.block} aria-label="Subsumption">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Subsumption</h3>
          <span className={classes.muted}>$subsumes</span>
        </header>
        <form
          className={classes.row}
          onSubmit={(event) => {
            event.preventDefault();
            if (codeA.trim() && codeB.trim()) check.mutate();
          }}
        >
          <TextInput className={classes.codeInput} aria-label="Code A" placeholder="Code A" value={codeA} onChange={setCodeA} />
          <TextInput className={classes.codeInput} aria-label="Code B" placeholder="Code B" value={codeB} onChange={setCodeB} />
          <Button type="submit" variant="outline" loading={check.isPending} disabled={!codeA.trim() || !codeB.trim()}>
            Compare
          </Button>
        </form>
        {check.isError && <OutcomeNote error={check.error} />}
        {check.data && (
          <ResultLine
            ok={parameterValue(check.data, "outcome") !== "not-subsumed"}
            label={SUBSUMPTION_LABELS[String(parameterValue(check.data, "outcome"))] ?? String(parameterValue(check.data, "outcome"))}
          />
        )}
      </section>
    </>
  );
}
