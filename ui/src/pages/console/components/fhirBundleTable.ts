// Derive a FHIR-aware tabular view from a search Bundle's resources.
// Picks meaningful columns and renders common FHIR datatypes as readable strings.

type FhirResource = Record<string, unknown> & { resourceType: string; id?: string };

const HIDDEN_KEYS = new Set([
  "resourceType",
  "id",
  "meta",
  "text",
  "implicitRules",
  "language",
  "contained",
  "extension",
  "modifierExtension",
]);

// Fields that make the most useful columns, in priority order.
const PREFERRED_KEYS = [
  "status",
  "name",
  "gender",
  "birthDate",
  "active",
  "code",
  "category",
  "subject",
  "patient",
  "encounter",
  "value",
  "valueQuantity",
  "valueString",
  "effectiveDateTime",
  "date",
  "period",
  "description",
  "title",
  "type",
  "identifier",
  "telecom",
  "address",
];

const MAX_DERIVED_COLUMNS = 6;

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function formatHumanName(n: Record<string, unknown>): string {
  if (typeof n.text === "string") return n.text;
  const given = Array.isArray(n.given)
    ? n.given.filter((g) => typeof g === "string").join(" ")
    : "";
  const family = typeof n.family === "string" ? n.family : "";
  return [given, family].filter(Boolean).join(" ").trim();
}

function formatScalarObject(o: Record<string, unknown>): string {
  // Reference
  if (typeof o.reference === "string") {
    return typeof o.display === "string" ? `${o.display} (${o.reference})` : o.reference;
  }
  if (typeof o.display === "string" && typeof o.system === "string") return o.display; // Coding
  // CodeableConcept
  if (Array.isArray(o.coding) && o.coding.length > 0) {
    const c = o.coding[0] as Record<string, unknown>;
    if (typeof o.text === "string") return o.text;
    if (typeof c?.display === "string") return c.display;
    if (typeof c?.code === "string") return c.code;
  }
  if (typeof o.text === "string") return o.text;
  // Quantity
  if (typeof o.value === "number") {
    const unit = typeof o.unit === "string" ? o.unit : typeof o.code === "string" ? o.code : "";
    return `${o.value}${unit ? ` ${unit}` : ""}`;
  }
  // Period
  if (typeof o.start === "string" || typeof o.end === "string") {
    return `${o.start ?? "…"} → ${o.end ?? "…"}`;
  }
  // HumanName
  if (typeof o.family === "string" || Array.isArray(o.given)) return formatHumanName(o);
  // Identifier / ContactPoint
  if (typeof o.value === "string") return o.value;
  if (typeof o.code === "string") return o.code;
  return "{…}";
}

/** Render any FHIR element value as a compact, readable cell string. */
export function formatFhirValue(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return "";
    // Array of HumanName
    if (isObject(value[0]) && ("family" in value[0] || "given" in value[0])) {
      return value.filter(isObject).map(formatHumanName).filter(Boolean).join("; ");
    }
    const parts = value
      .slice(0, 3)
      .map((v) => formatFhirValue(v))
      .filter(Boolean);
    const extra = value.length > 3 ? ` +${value.length - 3}` : "";
    return parts.join(", ") + extra;
  }
  if (isObject(value)) return formatScalarObject(value);
  return "";
}

export interface BundleColumn {
  key: string;
  header: string;
}

function titleize(key: string): string {
  return key
    .replace(/([A-Z])/g, " $1")
    .replace(/^./, (c) => c.toUpperCase())
    .trim();
}

/** Choose the most useful columns for a set of Bundle resources. */
export function deriveBundleColumns(resources: FhirResource[]): {
  columns: BundleColumn[];
  mixedTypes: boolean;
} {
  const types = new Set(resources.map((r) => r.resourceType));
  const mixedTypes = types.size > 1;

  const freq = new Map<string, number>();
  for (const r of resources) {
    for (const key of Object.keys(r)) {
      if (HIDDEN_KEYS.has(key)) continue;
      if (r[key] === undefined || r[key] === null) continue;
      freq.set(key, (freq.get(key) ?? 0) + 1);
    }
  }

  const candidates = [...freq.keys()];
  candidates.sort((a, b) => {
    const pa = PREFERRED_KEYS.indexOf(a);
    const pb = PREFERRED_KEYS.indexOf(b);
    const ra = pa === -1 ? Number.MAX_SAFE_INTEGER : pa;
    const rb = pb === -1 ? Number.MAX_SAFE_INTEGER : pb;
    if (ra !== rb) return ra - rb;
    return (freq.get(b) ?? 0) - (freq.get(a) ?? 0);
  });

  const columns: BundleColumn[] = candidates
    .slice(0, MAX_DERIVED_COLUMNS)
    .map((key) => ({ key, header: titleize(key) }));

  return { columns, mixedTypes };
}
