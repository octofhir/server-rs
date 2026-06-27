import { useEffect, useRef, useState } from "react";
import { useDebouncedValue } from "../hooks";
import type { ExpandValueSet, TerminologyConcept } from "./terminology";

export interface UseTerminologyExpansionOptions {
    expand: ExpandValueSet;
    valueSet?: string;
    count?: number;
    debounceMs?: number;
    minQueryLength?: number;
}

export interface TerminologyExpansionState {
    query: string;
    setQuery: (q: string) => void;
    concepts: TerminologyConcept[];
    loading: boolean;
}

/**
 * Debounced, race-safe `ValueSet/$expand` driver shared by the terminology
 * pickers. The newest request always wins; stale responses are dropped.
 */
export function useTerminologyExpansion({
    expand,
    valueSet,
    count = 20,
    debounceMs = 250,
    minQueryLength = 0,
}: UseTerminologyExpansionOptions): TerminologyExpansionState {
    const [query, setQuery] = useState("");
    const [concepts, setConcepts] = useState<TerminologyConcept[]>([]);
    const [loading, setLoading] = useState(false);
    const debouncedQuery = useDebouncedValue(query, debounceMs);
    const requestId = useRef(0);

    useEffect(() => {
        const q = debouncedQuery.trim();
        if (q.length < minQueryLength) {
            setConcepts([]);
            setLoading(false);
            return;
        }
        const id = ++requestId.current;
        setLoading(true);
        expand({ query: q, valueSet, count })
            .then((results) => {
                if (id === requestId.current) setConcepts(results);
            })
            .catch(() => {
                if (id === requestId.current) setConcepts([]);
            })
            .finally(() => {
                if (id === requestId.current) setLoading(false);
            });
    }, [debouncedQuery, minQueryLength, valueSet, count, expand]);

    return { query, setQuery, concepts, loading };
}
