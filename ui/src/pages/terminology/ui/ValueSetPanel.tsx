import { Button, DataTable, type DataTableColumn, TextInput, useDebouncedValue } from "@octofhir/ui-kit";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { type CanonicalHit, explainLines, type ExpansionConcept, expandValueSet, parameterValue, validateCode } from "../lib/api";
import { OutcomeNote, ResultLine } from "./OutcomeNote";
import classes from "./Terminology.module.css";

const PAGE = 100;

const COLUMNS: DataTableColumn<ExpansionConcept>[] = [
  {
    id: "code",
    header: "Code",
    width: 180,
    cell: (concept) => <span className={classes.code}>{concept.code}</span>,
  },
  {
    id: "display",
    header: "Display",
    cell: (concept) => (
      <span className={concept.inactive ? classes.muted : undefined}>
        {concept.display ?? "—"}
        {concept.inactive ? " (inactive)" : ""}
      </span>
    ),
  },
  {
    id: "system",
    header: "System",
    width: 280,
    cell: (concept) => <span className={classes.system}>{concept.system}</span>,
  },
];

/** First code system the value set pulls from, used to prefill "Check a code". */
function firstSystem(resource: Record<string, unknown> | undefined): string {
  const include = (resource?.compose as { include?: { system?: string }[] } | undefined)?.include;
  return include?.find((entry) => entry.system)?.system ?? "";
}

export function ValueSetPanel({ hit, resource }: { hit: CanonicalHit; resource?: Record<string, unknown> }) {
  const [filter, setFilter] = useState("");
  const [count, setCount] = useState(PAGE);
  const debounced = useDebouncedValue(filter, 250);
  const expansion = useQuery({
    queryKey: ["terminology", "expand", hit.url, debounced, count],
    queryFn: () => expandValueSet(hit.url, debounced, count),
    placeholderData: (previous) => previous,
    retry: false,
  });

  const [system, setSystem] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const check = useMutation({ mutationFn: () => validateCode(hit.url, system ?? firstSystem(resource), code) });
  const shown = expansion.data?.contains.length ?? 0;
  const total = expansion.data?.total;

  return (
    <>
      <section className={classes.block} aria-label="Expansion">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Expansion</h3>
          <span className={classes.muted}>
            {expansion.isError
              ? ""
              : total != null
                ? `${shown.toLocaleString()} of ${total.toLocaleString()} codes`
                : `${shown.toLocaleString()} codes`}
          </span>
          <TextInput
            className={classes.blockFilter}
            aria-label="Filter codes"
            placeholder="Filter codes…"
            value={filter}
            onChange={(value) => {
              setFilter(value);
              setCount(PAGE);
            }}
          />
        </header>
        {expansion.isError ? (
          <OutcomeNote error={expansion.error} />
        ) : (
          <>
            <DataTable
              aria-label="Expanded codes"
              data={expansion.data?.contains ?? []}
              columns={COLUMNS}
              getRowId={(concept) => `${concept.system}|${concept.code}`}
              loading={expansion.isPending}
              loadingRowCount={6}
              maxHeight={420}
              virtualized
              emptyState={debounced ? `No codes match “${debounced}”.` : "The expansion is empty."}
            />
            {total != null && shown < total && (
              <Button
                className={classes.more}
                variant="subtle"
                size="sm"
                loading={expansion.isFetching}
                onClick={() => setCount((value) => value + PAGE)}
              >
                Show {Math.min(PAGE, total - shown)} more
              </Button>
            )}
          </>
        )}
      </section>

      <section className={classes.block} aria-label="Check a code">
        <header className={classes.blockHeader}>
          <h3 className={classes.blockTitle}>Check a code</h3>
          <span className={classes.muted}>$validate-code</span>
        </header>
        <form
          className={classes.row}
          onSubmit={(event) => {
            event.preventDefault();
            if (code.trim()) check.mutate();
          }}
        >
          <TextInput
            className={classes.systemInput}
            aria-label="Code system"
            placeholder="System URL"
            value={system ?? firstSystem(resource)}
            onChange={setSystem}
          />
          <TextInput className={classes.codeInput} aria-label="Code" placeholder="Code" value={code} onChange={setCode} />
          <Button type="submit" variant="outline" loading={check.isPending} disabled={!code.trim()}>
            Check
          </Button>
        </form>
        {check.isError && <OutcomeNote error={check.error} />}
        {check.data && (
          <ResultLine
            ok={parameterValue(check.data, "result") === true}
            label={parameterValue(check.data, "result") === true ? "In the value set" : "Not in the value set"}
            detail={String(parameterValue(check.data, "display") ?? parameterValue(check.data, "message") ?? "")}
          />
        )}
        {check.data &&
          explainLines(check.data).map((line) => (
            <p key={line} className={classes.muted}>
              {line}
            </p>
          ))}
      </section>
    </>
  );
}
