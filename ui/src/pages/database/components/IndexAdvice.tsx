import { Button, notify, StatusBadge, type StatusTone, TextArea } from "@octofhir/ui-kit";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Copy, SquareTerminal } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { fhirClient } from "@/shared/api/fhirClient";
import { useResourceTypesCategorized } from "@/shared/api/hooks";
import classes from "./TableMetaDetail.module.css";

interface IndexSuggestion {
  id: string;
  category: string;
  priority: "high" | "medium" | "low" | string;
  reason: string;
  tradeoff: string;
  createStatement?: string | null;
  existingIndex?: string | null;
}

interface ObservedQuery {
  source: string;
  query: string;
  calls?: number | null;
  meanTimeMs?: number | null;
}

interface AdvisorResponse {
  suggestions: IndexSuggestion[];
  observedQueries: ObservedQuery[];
  notes: string[];
}

const PRIORITY_TONE: Record<string, StatusTone> = { high: "warning", medium: "info", low: "neutral" };

async function advise(resourceType: string): Promise<AdvisorResponse> {
  const response = await fhirClient.rawRequest(
    "GET",
    `/api/db-console/index-advisor?resourceType=${encodeURIComponent(resourceType)}&limit=10`
  );
  return response.data as AdvisorResponse;
}

async function analyze(resourceType: string, queries: string[]): Promise<AdvisorResponse> {
  const response = await fhirClient.rawRequest("POST", "/api/db-console/index-advisor/analyze", {
    resourceType,
    queries,
  });
  return response.data as AdvisorResponse;
}

/**
 * Advice for one resource table: from the table's shape and observed SQL by default, or from
 * request URLs the user pastes. Read-only: suggested DDL opens in the SQL console to be run by hand.
 */
export function IndexAdvice({ table }: { table: string }) {
  const navigate = useNavigate();
  // The advisor matches search parameters by resource type name (`Observation`), not table name.
  const { data: catalog } = useResourceTypesCategorized();
  const resourceType = catalog?.types.find((type) => type.name.toLowerCase() === table)?.name;
  const general = useQuery({
    queryKey: ["index-advisor", resourceType],
    queryFn: () => advise(resourceType ?? table),
    enabled: resourceType != null,
    staleTime: 60_000,
  });
  const [urls, setUrls] = useState("");
  const shaped = useMutation({
    mutationFn: () =>
      analyze(
        resourceType ?? table,
        urls
          .split("\n")
          .map((line) => line.trim())
          .filter(Boolean)
      ),
    onError: (error) =>
      notify({ theme: "danger", title: "Analysis failed", content: error instanceof Error ? error.message : undefined }),
  });
  const result = shaped.data ?? general.data;
  if (!resourceType) return null;

  const copy = (sql: string) =>
    void navigator.clipboard
      .writeText(sql)
      .then(() => notify({ theme: "success", title: "Copied CREATE INDEX", autoHiding: 2000 }));

  return (
    <section>
      <div className={classes.sectionHead}>
        <span className={classes.sectionTitle}>Index advice</span>
        <span className={classes.sectionCount}>{result?.suggestions.length ?? 0}</span>
      </div>

      <div className={classes.adviceForm}>
        <TextArea
          aria-label="FHIR request URLs to analyze"
          placeholder={"Paste request URLs, one per line\n/fhir/Observation?code=http://loinc.org|8867-4&date=ge2024"}
          value={urls}
          onChange={setUrls}
          minRows={2}
          maxRows={6}
        />
        <div className={classes.adviceActions}>
          <Button size="xs" variant="outline" loading={shaped.isPending} disabled={!urls.trim()} onClick={() => shaped.mutate()}>
            Analyze requests
          </Button>
          {shaped.data && (
            <Button size="xs" variant="subtle" onClick={() => shaped.reset()}>
              Back to table advice
            </Button>
          )}
        </div>
      </div>

      {general.isError ? (
        <div className={classes.empty}>Index advice needs the DB console to be enabled.</div>
      ) : result && result.suggestions.length === 0 ? (
        <div className={classes.empty}>No suggestions: the table is covered for these access paths.</div>
      ) : (
        <div className={classes.indexList}>
          {result?.suggestions.map((suggestion) => (
            <div key={suggestion.id} className={classes.indexCard}>
              <div className={classes.adviceTop}>
                <StatusBadge tone={PRIORITY_TONE[suggestion.priority] ?? "neutral"}>{suggestion.priority}</StatusBadge>
                <span className={classes.adviceCategory}>{suggestion.category}</span>
              </div>
              <p className={classes.adviceReason}>{suggestion.reason}</p>
              <p className={classes.adviceTradeoff}>{suggestion.tradeoff}</p>
              {suggestion.existingIndex && (
                <p className={classes.adviceTradeoff}>Covered by {suggestion.existingIndex}</p>
              )}
              {suggestion.createStatement && (
                <>
                  <pre className={classes.indexDef}>{suggestion.createStatement}</pre>
                  <div className={classes.adviceActions}>
                    <Button size="xs" variant="subtle" onClick={() => copy(suggestion.createStatement ?? "")}>
                      <Button.Icon side="left">
                        <Copy size={12} />
                      </Button.Icon>
                      Copy SQL
                    </Button>
                    <Button
                      size="xs"
                      variant="subtle"
                      onClick={() =>
                        navigate(`/db-console?${new URLSearchParams({ sql: suggestion.createStatement ?? "" }).toString()}`)
                      }
                    >
                      <Button.Icon side="left">
                        <SquareTerminal size={12} />
                      </Button.Icon>
                      Open in SQL
                    </Button>
                  </div>
                </>
              )}
            </div>
          ))}
        </div>
      )}

      {(result?.observedQueries.length ?? 0) > 0 && (
        <div className={classes.indexList}>
          {result?.observedQueries.map((query) => (
            <div key={query.query} className={classes.indexCard}>
              <span className={classes.adviceCategory}>
                {query.calls != null ? `${query.calls.toLocaleString()} calls` : query.source}
                {query.meanTimeMs != null ? ` · ${query.meanTimeMs.toFixed(1)} ms mean` : ""}
              </span>
              <pre className={classes.indexDef}>{query.query}</pre>
            </div>
          ))}
        </div>
      )}

      {result?.notes.map((note) => (
        <p key={note} className={classes.adviceNote}>
          {note}
        </p>
      ))}
    </section>
  );
}
