/** FHIR `Coding` datatype (subset). */
export interface Coding {
    system?: string;
    code?: string;
    display?: string;
    version?: string;
}

/** FHIR `CodeableConcept` datatype (subset). */
export interface CodeableConcept {
    coding?: Coding[];
    text?: string;
}

/** A concept returned by a `ValueSet/$expand` operation. */
export interface TerminologyConcept {
    system?: string;
    code: string;
    display?: string;
}

export interface ExpandParams {
    /** The user's free-text filter (maps to `$expand?filter=`). */
    query: string;
    /** Canonical URL of the ValueSet to expand. */
    valueSet?: string;
    /** Page size (maps to `$expand?count=`). */
    count?: number;
}

/** Caller-supplied terminology expansion; wire to `GET /ValueSet/$expand`. */
export type ExpandValueSet = (params: ExpandParams) => Promise<TerminologyConcept[]>;

/** Stable key for a coding, used as the option value in pickers. */
export function codingKey(c: { system?: string; code?: string }): string {
    return `${c.system ?? ""}|${c.code ?? ""}`;
}
