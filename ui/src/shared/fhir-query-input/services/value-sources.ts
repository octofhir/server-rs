import { getResourceDisplayName } from "@octofhir/ui-kit";
import { fhirClient, serverApi } from "@/shared/api";
import type { CursorContext, QueryInputMetadata, QuerySuggestion } from "../core/types";

/** Reference targets searched per completion; more would make the popup slow and noisy. */
const MAX_REFERENCE_TARGETS = 3;
const REFERENCES_PER_TARGET = 8;

type ElementValues = { kind: "codes"; codes: string[] } | { kind: "boolean" } | null;

const schemaCache = new Map<string, Promise<Record<string, unknown> | null>>();
const referenceCache = new Map<string, Promise<QuerySuggestion[]>>();

/**
 * Values that need the server: codes of the element a token parameter searches
 * (read from the resource's JSON Schema) and existing resources for reference parameters.
 */
export async function resolveValueSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): Promise<{ suggestions: QuerySuggestion[]; incomplete: boolean }> {
  const none = { suggestions: [], incomplete: false };
  const { resourceType, paramName, modifier } = context;
  if (!resourceType || !paramName || paramName.startsWith("_") || modifier) return none;
  const param = metadata.searchParamsByResource[resourceType]?.find((p) => p.code === paramName);
  if (!param) return none;
  const fragment = currentValue(context.fragment);

  if (param.type === "token") {
    const values = await elementValues(resourceType, paramName);
    if (values?.kind === "boolean") {
      return { suggestions: staticValues(["true", "false"], "boolean"), incomplete: false };
    }
    if (values?.kind === "codes") {
      return {
        suggestions: staticValues(values.codes, `${resourceType}.${paramName}`),
        incomplete: false,
      };
    }
    return none;
  }
  if (param.type === "reference" && param.targets.length) {
    const targets = param.targets.slice(0, MAX_REFERENCE_TARGETS);
    const lists = await Promise.all(
      targets.map((target) => referenceSuggestions(target, fragment, metadata))
    );
    // Results depend on what is typed (name search), so ask again on every keystroke.
    return { suggestions: lists.flat(), incomplete: true };
  }
  return none;
}

/** The value being typed: after the last comma (comma = OR). */
function currentValue(fragment: string): string {
  return fragment.slice(fragment.lastIndexOf(",") + 1);
}

function staticValues(values: string[], detail: string): QuerySuggestion[] {
  return values.map((value, index) => ({
    label: value,
    insertText: value,
    kind: "value",
    detail,
    sortPriority: index,
  }));
}

async function elementValues(resourceType: string, paramName: string): Promise<ElementValues> {
  const schema = await resourceSchema(resourceType);
  const properties = schema?.properties as Record<string, Record<string, unknown>> | undefined;
  if (!properties) return null;
  const camel = paramName.replace(/-([a-z])/g, (_, ch: string) => ch.toUpperCase());
  const raw = properties[camel] ?? properties[paramName];
  const property = (raw?.type === "array" ? raw.items : raw) as Record<string, unknown> | undefined;
  if (!property) return null;
  if (property.type === "boolean") return { kind: "boolean" };
  // Required code bindings carry their codes in the short description: "male | female | other".
  if (property.type === "string" && typeof property.description === "string") {
    const codes = property.description
      .replace(/\s*\+$/, "")
      .split(" | ")
      .map((code) => code.trim());
    if (codes.length > 1 && codes.every((code) => /^[A-Za-z0-9.-]+$/.test(code))) {
      return { kind: "codes", codes };
    }
  }
  return null;
}

function resourceSchema(resourceType: string): Promise<Record<string, unknown> | null> {
  let pending = schemaCache.get(resourceType);
  if (!pending) {
    pending = serverApi
      .getJsonSchema(resourceType)
      .then((schema) => (schema && typeof schema === "object" ? (schema as Record<string, unknown>) : null))
      .catch(() => null);
    schemaCache.set(resourceType, pending);
  }
  return pending;
}

function referenceSuggestions(
  target: string,
  fragment: string,
  metadata: QueryInputMetadata
): Promise<QuerySuggestion[]> {
  // `Practitioner/abc` already names the type: filter on the part after the slash.
  const typed = fragment.includes("/") ? fragment.slice(fragment.indexOf("/") + 1) : fragment;
  const hasName = metadata.searchParamsByResource[target]?.some((p) => p.code === "name");
  const params: Record<string, string | number> = {
    _count: REFERENCES_PER_TARGET,
    _sort: "-_lastUpdated",
  };
  if (typed.length >= 2 && hasName && !fragment.includes("/")) params.name = typed;
  const key = `${target}?${new URLSearchParams(params as Record<string, string>).toString()}`;

  let pending = referenceCache.get(key);
  if (!pending) {
    pending = fhirClient
      .search(target, params)
      .then((bundle) =>
        (bundle.entry ?? []).flatMap((entry, index) => {
          const resource = entry.resource;
          if (!resource?.id) return [];
          const address = `${target}/${resource.id}`;
          const name = getResourceDisplayName(resource, "");
          // People pick by name; the address is what gets inserted.
          return [
            {
              label: name || address,
              insertText: address,
              filterText: `${name} ${address}`,
              kind: "value" as const,
              detail: target,
              description: name ? address : undefined,
              sortPriority: index,
            },
          ];
        })
      )
      .catch(() => []);
    referenceCache.set(key, pending);
    // Data changes; a short-lived cache only spares the server while the user keeps typing.
    setTimeout(() => referenceCache.delete(key), 30_000);
  }
  return pending;
}
