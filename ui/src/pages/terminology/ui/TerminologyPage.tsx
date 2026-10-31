import { EmptyState, SegmentedRadioGroup, Skeleton, TextInput, useDebouncedValue } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { BookOpen, Search } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { usePackages } from "@/shared/api/hooks";
import { useUrlFilters } from "@/shared/lib/url-filters";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { type CanonicalHit, fetchCanonical, SEARCH_LIMIT, searchCanonicals, type TerminologyKind } from "../lib/api";
import { CodeSystemPanel } from "./CodeSystemPanel";
import { ConceptMapPanel } from "./ConceptMapPanel";
import { OutcomeNote } from "./OutcomeNote";
import classes from "./Terminology.module.css";
import { ValueSetPanel } from "./ValueSetPanel";

const KINDS: { value: TerminologyKind; label: string }[] = [
  { value: "ValueSet", label: "Value sets" },
  { value: "CodeSystem", label: "Code systems" },
  { value: "ConceptMap", label: "Maps" },
];

const URL_KEYS = { text: ["kind", "q", "url", "pkg"] } as const;

function isKind(value: string | undefined): value is TerminologyKind {
  return value === "ValueSet" || value === "CodeSystem" || value === "ConceptMap";
}

/** `pkg` is kept in the URL as `name@version`. */
function selectedHit(kind: TerminologyKind, url?: string, pkg?: string): CanonicalHit | null {
  if (!url || !pkg) return null;
  const at = pkg.lastIndexOf("@");
  if (at <= 0) return null;
  return { kind, url, name: url.split("/").pop() ?? url, packageName: pkg.slice(0, at), packageVersion: pkg.slice(at + 1) };
}

function Detail({ hit }: { hit: CanonicalHit }) {
  const resource = useQuery({
    queryKey: ["terminology", "resource", hit.packageName, hit.packageVersion, hit.url],
    queryFn: () => fetchCanonical(hit),
    staleTime: Number.POSITIVE_INFINITY,
  });
  const data = resource.data;
  const title = typeof data?.title === "string" ? data.title : typeof data?.name === "string" ? data.name : hit.name;
  const description = typeof data?.description === "string" ? data.description.split("\n")[0] : undefined;

  return (
    <div className={classes.detail}>
      <header className={classes.detailHeader}>
        <h2 className={classes.detailTitle}>{title}</h2>
        <code className={classes.detailUrl}>{hit.url}</code>
        <div className={classes.detailMeta}>
          {typeof data?.version === "string" && <span>v{data.version}</span>}
          {typeof data?.status === "string" && <span>{data.status}</span>}
          <span className={classes.packageChip}>
            {hit.packageName}@{hit.packageVersion}
          </span>
        </div>
        {description && <p className={classes.description}>{description}</p>}
      </header>
      {resource.isPending ? (
        <Skeleton className={classes.detailSkeleton} />
      ) : resource.isError ? (
        <OutcomeNote error={resource.error} />
      ) : hit.kind === "ValueSet" ? (
        <ValueSetPanel key={hit.url} hit={hit} resource={data} />
      ) : hit.kind === "CodeSystem" ? (
        <CodeSystemPanel key={hit.url} hit={hit} resource={data} />
      ) : (
        <ConceptMapPanel key={hit.url} hit={hit} resource={data} />
      )}
    </div>
  );
}

export function TerminologyPage() {
  const [filters, setFilters] = useUrlFilters<{ kind?: string; q?: string; url?: string; pkg?: string }>(URL_KEYS);
  const kind: TerminologyKind = isKind(filters.kind) ? filters.kind : "ValueSet";
  const [query, setQuery] = useState(filters.q ?? "");
  const debounced = useDebouncedValue(query, 250);
  const { data: packages } = usePackages();

  // Only the settled query goes to the URL; the setter is read through a ref so a
  // URL change (which recreates it) does not write the query back again.
  const setFiltersRef = useRef(setFilters);
  setFiltersRef.current = setFilters;
  useEffect(() => {
    setFiltersRef.current({ q: debounced.trim() || undefined });
  }, [debounced]);

  const search = useQuery({
    queryKey: ["terminology", "search", kind, debounced.trim(), packages?.packages.map((pkg) => `${pkg.name}@${pkg.version}`)],
    queryFn: () => searchCanonicals(packages?.packages ?? [], kind, debounced),
    enabled: packages != null,
    placeholderData: (previous) => previous,
  });
  const hits = search.data?.hits ?? [];
  const selected = selectedHit(kind, filters.url, filters.pkg);
  const isSelected = (hit: CanonicalHit) =>
    selected?.url === hit.url && selected.packageName === hit.packageName && selected.packageVersion === hit.packageVersion;
  const select = (hit: CanonicalHit) => setFilters({ url: hit.url, pkg: `${hit.packageName}@${hit.packageVersion}` });

  const move = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const index = hits.findIndex(isSelected);
    const next = hits[index < 0 ? 0 : index + (event.key === "ArrowDown" ? 1 : -1)];
    if (!next) return;
    select(next);
    document.querySelector<HTMLElement>(`[data-hit="${CSS.escape(`${next.packageName}|${next.url}`)}"]`)?.scrollIntoView({
      block: "nearest",
    });
  };

  return (
    <ToolWorkspaceLayout title="Terminology" description="Value sets, code systems and concept maps from installed packages.">
      <div className={classes.layout}>
        <aside className={classes.sidebar} aria-label="Terminology resources">
          <SegmentedRadioGroup
            aria-label="Resource kind"
            size="sm"
            fullWidth
            value={kind}
            onChange={(value) => setFilters({ kind: value === "ValueSet" ? undefined : value, url: undefined, pkg: undefined })}
            options={KINDS}
          />
          <TextInput
            aria-label="Search by name or URL"
            placeholder="Search by name or URL…"
            leftSection={<Search size={14} aria-hidden="true" />}
            value={query}
            onChange={setQuery}
            onKeyDown={move}
          />
          <span className={classes.muted}>
            {search.isPending
              ? "Searching packages…"
              : search.data && search.data.total > hits.length
                ? `First ${hits.length} of ${search.data.total.toLocaleString()}`
                : `${hits.length} found`}
          </span>
          <ul className={classes.hits} onKeyDown={move}>
            {hits.map((hit) => (
              <li key={`${hit.packageName}|${hit.url}`}>
                <button
                  type="button"
                  className={classes.hit}
                  aria-pressed={isSelected(hit)}
                  data-hit={`${hit.packageName}|${hit.url}`}
                  onClick={() => select(hit)}
                >
                  <span className={classes.hitName}>{hit.name}</span>
                  <span className={classes.hitUrl}>{hit.url}</span>
                  <span className={classes.hitPackage}>{hit.packageName}</span>
                </button>
              </li>
            ))}
            {hits.length === SEARCH_LIMIT && <li className={classes.muted}>Refine the search to see more.</li>}
          </ul>
        </aside>
        <main className={classes.main}>
          {selected ? (
            <Detail key={`${selected.packageName}|${selected.url}`} hit={hits.find(isSelected) ?? selected} />
          ) : (
            <EmptyState
              size="promo"
              image={<BookOpen width={32} height={32} aria-hidden="true" />}
              title="Pick a terminology resource"
              description="Expand value sets, browse code systems with $lookup, and translate codes through concept maps."
            />
          )}
        </main>
      </div>
    </ToolWorkspaceLayout>
  );
}
