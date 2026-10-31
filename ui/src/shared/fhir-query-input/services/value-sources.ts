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
): Promise<QuerySuggestion[]> {
  const { resourceType, paramName, modifier } = context;
  if (!resourceType || !paramName || paramName.startsWith("_") || modifier) return [];
  const param = metadata.searchParamsByResource[resourceType]?.find((p) => p.code === paramName);
  if (!param) return [];
  const fragment = currentValue(context.fragment);

  if (param.type === "token") {
    const values = await elementValues(resourceType, paramName);
    if (values?.kind === "boolean") return staticValues(["true", "false"], "boolean");
    if (values?.kind === "codes") return staticValues(values.codes, `${resourceType}.${paramName}`);
    return [];
  }
  if (param.type === "reference" && param.targets.length) {
    const targets = param.targets.slice(0, MAX_REFERENCE_TARGETS);
    const lists = await Promise.all(
      targets.map((target) => resourceSuggestions(target, fragment, metadata, "address"))
    );
    return lists.flat();
  }
  return [];
}

/** `Patient/` lists existing patients by name; the id is what gets inserted. */
export function resolveResourceIdSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): Promise<QuerySuggestion[]> {
  if (!context.resourceType) return Promise.resolve([]);
  return resourceSuggestions(context.resourceType, context.fragment, metadata, "id");
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

function resourceSuggestions(
  target: string,
  fragment: string,
  metadata: QueryInputMetadata,
  insert: "address" | "id"
): Promise<QuerySuggestion[]> {
  // Letters are a name to search for; ids (digits, dashes) are filtered client-side.
  const typingAddress = fragment.includes("/");
  const hasName = metadata.searchParamsByResource[target]?.some((p) => p.code === "name");
  const params: Record<string, string | number> = {
    _count: REFERENCES_PER_TARGET,
    _sort: "-_lastUpdated",
  };
  if (hasName && /^[\p{L} ]{2,}$/u.test(fragment)) params.name = fragment;
  const key = `${insert}:${target}?${new URLSearchParams(params as Record<string, string>).toString()}`;

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
          // People pick by name; the address (or bare id) is what gets inserted.
          return [
            {
              label: name || address,
              insertText: insert === "id" ? resource.id : address,
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
  // Monaco matches what was typed against filterText: lead with whichever the user is typing.
  return pending.then((items) =>
    items.map((item) => {
      const name = item.description ? item.label : "";
      const address = item.description ?? item.label;
      return { ...item, filterText: typingAddress ? `${address} ${name}` : `${name} ${address}` };
    })
  );
}
