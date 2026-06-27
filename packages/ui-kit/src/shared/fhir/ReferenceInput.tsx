import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useDebouncedValue } from "../hooks";
import { Combobox, type ComboboxData } from "../ui";
import { parseFhirReference } from "./ReferenceLink";
import { ResourceTypeBadge } from "./ResourceTypeBadge";
import classes from "./ReferenceInput.module.css";

/** Minimal FHIR `Reference` shape produced/consumed by the picker. */
export interface FhirReference {
    reference: string;
    display?: string;
    type?: string;
}

/** A search hit returned by the caller-supplied {@link ReferenceInputProps.search} function. */
export interface ReferenceCandidate {
    /** Relative reference, e.g. `"Patient/123"`. */
    reference: string;
    display?: string;
    resourceType?: string;
    id?: string;
    /** Optional secondary line (identifier, birth date, …). */
    secondary?: ReactNode;
}

export interface ReferenceInputProps {
    value?: FhirReference | null;
    onChange?: (value: FhirReference | null) => void;
    /** Restrict search to one or more resource types. */
    resourceType?: string | string[];
    /**
     * Performs the server search. Wire this to your FHIR client
     * (e.g. `GET /fhir/{type}?_text=…` or `name:contains=…`).
     */
    search: (params: { query: string; resourceType?: string | string[] }) => Promise<ReferenceCandidate[]>;
    /** Debounce before firing {@link search}. Default 250ms. */
    debounceMs?: number;
    /** Minimum query length before searching. Default 1. */
    minQueryLength?: number;

    placeholder?: string;
    disabled?: boolean;
    required?: boolean;
    clearable?: boolean;
    size?: "xs" | "sm" | "md" | "lg" | "xl";

    label?: ReactNode;
    description?: ReactNode;
    error?: boolean | string;
    w?: number | string;
    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

function candidateResourceType(c: ReferenceCandidate): string | undefined {
    return c.resourceType ?? parseFhirReference(c.reference).resourceType;
}

export function ReferenceInput({
    value,
    onChange,
    resourceType,
    search,
    debounceMs = 250,
    minQueryLength = 1,
    placeholder = "Search…",
    clearable = true,
    ...rest
}: ReferenceInputProps) {
    const [query, setQuery] = useState("");
    const [candidates, setCandidates] = useState<ReferenceCandidate[]>([]);
    const [loading, setLoading] = useState(false);
    const debouncedQuery = useDebouncedValue(query, debounceMs);
    const requestId = useRef(0);

    useEffect(() => {
        const q = debouncedQuery.trim();
        if (q.length < minQueryLength) {
            setCandidates([]);
            setLoading(false);
            return;
        }
        const id = ++requestId.current;
        setLoading(true);
        search({ query: q, resourceType })
            .then((results) => {
                if (id === requestId.current) setCandidates(results);
            })
            .catch(() => {
                if (id === requestId.current) setCandidates([]);
            })
            .finally(() => {
                if (id === requestId.current) setLoading(false);
            });
    }, [debouncedQuery, minQueryLength, resourceType, search]);

    // Index candidates by reference, and keep the current value selectable even
    // when it is not part of the latest result set.
    const byRef = useMemo(() => {
        const map = new Map<string, ReferenceCandidate>();
        for (const c of candidates) map.set(c.reference, c);
        if (value?.reference && !map.has(value.reference)) {
            map.set(value.reference, {
                reference: value.reference,
                display: value.display,
                resourceType: value.type,
            });
        }
        return map;
    }, [candidates, value]);

    const data = useMemo<ComboboxData>(() => {
        return Array.from(byRef.values()).map((c) => {
            const type = candidateResourceType(c);
            const display = c.display ?? c.reference;
            return {
                value: c.reference,
                textValue: display,
                label: (
                    <span className={classes.option}>
                        {type && <ResourceTypeBadge resourceType={type} />}
                        <span className={classes.optionBody}>
                            <span className={classes.optionTitle}>{display}</span>
                            {c.secondary != null ? (
                                <span className={classes.optionSecondary}>{c.secondary}</span>
                            ) : (
                                <span className={classes.optionRef}>{c.reference}</span>
                            )}
                        </span>
                    </span>
                ),
            };
        });
    }, [byRef]);

    return (
        <Combobox
            {...rest}
            data={data}
            placeholder={placeholder}
            clearable={clearable}
            filter="server"
            loading={loading}
            emptyMessage={query.trim().length < minQueryLength ? "Type to search" : "No matches"}
            value={value?.reference ?? null}
            onInputChange={setQuery}
            onChange={(ref) => {
                if (!ref) {
                    onChange?.(null);
                    return;
                }
                const c = byRef.get(ref);
                onChange?.({
                    reference: ref,
                    display: c?.display,
                    type: c ? candidateResourceType(c) : parseFhirReference(ref).resourceType,
                });
            }}
        />
    );
}
