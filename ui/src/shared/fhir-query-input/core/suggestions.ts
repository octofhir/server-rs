import type { ResourceCapability, RestConsoleSearchParam } from "@/shared/api";
import { SEARCH_MODIFIERS } from "./search-key";
import type { CursorContext, QueryInputMetadata, QuerySuggestion } from "./types";

const MAX_SUGGESTIONS = 20;

/** Strip leading "/" from operation labels — the range calculation handles separators */
function stripLeadingSlash(s: string): string {
  return s.startsWith("/") ? s.slice(1) : s;
}

/** Ensure leading "/" for zero-width insertions after resource/id */
function ensureLeadingSlash(s: string): string {
  return s.startsWith("/") || s.startsWith("?") ? s : `/${s}`;
}

export function getSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  switch (context.type) {
    case "root":
      return getRootSuggestions();
    case "api-endpoint":
      return getApiEndpointSuggestions(context, metadata);
    case "resource-type":
      return getResourceTypeSuggestions(context, metadata);
    case "next-after-resource":
      return getNextAfterResourceSuggestions(context, metadata);
    case "next-after-id":
      return getNextAfterIdSuggestions(context, metadata);
    case "resource-id":
      return getResourceIdSuggestions();
    case "type-operation":
      return getOperationSuggestions(context, metadata, "type-op");
    case "instance-operation":
      return getOperationSuggestions(context, metadata, "instance-op");
    case "query-param":
      return getQueryParamSuggestions(context, metadata);
    case "query-modifier":
      return getQueryModifierSuggestions(context, metadata);
    case "system-operation":
      return getSystemOperationSuggestions(context, metadata);
    case "query-value":
      return getQueryValueSuggestions(context, metadata);
    default:
      return [];
  }
}

function getRootSuggestions(): QuerySuggestion[] {
  return [
    {
      label: "/fhir",
      insertText: "/fhir",
      kind: "structural",
      detail: "FHIR API base path",
      sortPriority: 0,
    },
    {
      label: "/api",
      insertText: "/api",
      kind: "structural",
      detail: "Internal API endpoints",
      sortPriority: 1,
    },
  ];
}

function getApiEndpointSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const fragment = context.fragment.toLowerCase();
  return metadata.allSuggestions
    .filter((s) => s.kind === "api-endpoint" && s.label.toLowerCase().includes(fragment))
    .map((s) => ({
      label: s.label,
      insertText: s.path_template,
      kind: "api-endpoint" as const,
      detail: s.description || s.methods.join(", "),
      sortPriority: 0,
    }));
}

function getResourceTypeSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const fragment = context.fragment.toLowerCase();

  // System operations — strip leading "/" since range replaces from after "/"
  const systemOps = metadata.allSuggestions
    .filter((s) => s.kind === "system-op" && s.label.toLowerCase().includes(fragment))
    .map((s) => ({
      label: s.label,
      insertText: stripLeadingSlash(s.label),
      kind: "operation" as const,
      detail: s.description || `${s.methods.join(", ")} - System operation`,
      sortPriority: 0,
    }));

  // Resource types
  const resources = metadata.allSuggestions
    .filter((s) => s.kind === "resource" && s.label.toLowerCase().includes(fragment))
    .slice(0, MAX_SUGGESTIONS)
    .map((s) => ({
      label: s.label,
      insertText: s.label,
      kind: "resource" as const,
      detail: s.description,
      sortPriority: 1,
    }));

  return [...systemOps, ...resources];
}

function getNextAfterResourceSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const resourceType = context.resourceType;
  const results: QuerySuggestion[] = [
    {
      label: "/{id}",
      insertText: "/{id}",
      kind: "structural",
      detail: "Read specific resource by ID",
      sortPriority: 0,
    },
    {
      label: "?",
      insertText: "?",
      kind: "structural",
      detail: "Search with query parameters",
      sortPriority: 1,
    },
  ];

  // Type-level operations — ensure leading "/" for zero-width insertion
  if (resourceType) {
    const typeOps = metadata.allSuggestions
      .filter((s) => s.kind === "type-op" && s.metadata.resource_type === resourceType)
      .map((s) => ({
        label: s.label,
        insertText: ensureLeadingSlash(s.label),
        kind: "operation" as const,
        detail: s.description || `${s.methods.join(", ")} - Type operation`,
        sortPriority: 2,
      }));
    results.push(...typeOps);
  }

  return results;
}

function getNextAfterIdSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const resourceType = context.resourceType;
  if (!resourceType) return [];

  return metadata.allSuggestions
    .filter((s) => s.kind === "instance-op" && s.metadata.resource_type === resourceType)
    .map((s) => ({
      label: s.label,
      insertText: ensureLeadingSlash(s.label),
      kind: "operation" as const,
      detail: s.description || `${s.methods.join(", ")} - Instance operation`,
      sortPriority: 0,
    }));
}

function getResourceIdSuggestions(): QuerySuggestion[] {
  return [
    {
      label: "{id}",
      insertText: "{id}",
      kind: "structural",
      detail: "Enter resource ID",
      sortPriority: 0,
    },
  ];
}

function getOperationSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata,
  opKind: "type-op" | "instance-op"
): QuerySuggestion[] {
  const resourceType = context.resourceType;
  const fragment = context.fragment.replace("$", "").toLowerCase();

  return metadata.allSuggestions
    .filter(
      (s) =>
        s.kind === opKind &&
        s.metadata.resource_type === resourceType &&
        s.label.toLowerCase().includes(fragment)
    )
    .map((s) => ({
      label: s.label,
      insertText: stripLeadingSlash(s.label),
      kind: "operation" as const,
      detail: s.description || s.methods.join(", "),
      sortPriority: 0,
    }));
}

function getSystemOperationSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const fragment = context.fragment.replace("$", "").toLowerCase();

  return metadata.allSuggestions
    .filter((s) => s.kind === "system-op" && s.label.toLowerCase().includes(fragment))
    .map((s) => ({
      label: s.label,
      insertText: stripLeadingSlash(s.label),
      kind: "operation" as const,
      detail: s.description || `${s.methods.join(", ")} - System operation`,
      sortPriority: 0,
    }));
}

function getQueryParamSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const resourceType = context.resourceType;
  if (!resourceType) return [];

  const params = metadata.searchParamsByResource[resourceType] || [];
  const fragment = context.fragment.toLowerCase();

  const paramSuggestions: QuerySuggestion[] = params
    .filter((p) => p.code.toLowerCase().includes(fragment))
    .slice(0, 15)
    .map((p) => ({
      label: p.code,
      insertText: `${p.code}=`,
      kind: "param" as const,
      detail: p.type,
      sortPriority: p.is_common ? 1 : 0,
    }));

  // Add special params from capabilities
  const specialSuggestions = getSpecialParamSuggestions(fragment, metadata);

  return [...paramSuggestions, ...specialSuggestions];
}

function getSpecialParamSuggestions(
  fragment: string,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  if (!metadata.capabilities) return [];

  return metadata.capabilities.special_params
    .filter((sp) => sp.supported && sp.name.toLowerCase().includes(fragment))
    .map((sp) => ({
      label: sp.name,
      insertText: `${sp.name}=`,
      kind: "special" as const,
      detail: sp.description,
      sortPriority: 2,
    }));
}

function getQueryModifierSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const resourceType = context.resourceType;
  const paramName = context.paramName;
  if (!resourceType || !paramName) return [];

  const byType: Record<string, RestConsoleSearchParam[]> = metadata.searchParamsByResource;
  const rootDef = (byType[resourceType] || []).find((p) => p.code === paramName);
  if (!rootDef) return [];

  const knownTypes = new Set(metadata.resourceTypes);
  // `context.fragment` is everything after the first ':' (may include a chain).
  const parts = context.fragment.split(".");
  const isReference = rootDef.type.toLowerCase() === "reference";

  // ── Right after `param:` — offer type modifiers (reference targets) and
  //    search modifiers side by side. ─────────────────────────────────────
  if (parts.length === 1) {
    const frag = parts[0].toLowerCase();
    const out: QuerySuggestion[] = [];
    if (isReference) {
      for (const t of rootDef.targets ?? []) {
        if (t.toLowerCase().includes(frag)) {
          out.push({
            label: `:${t}`,
            insertText: `${t}.`,
            filterText: t,
            kind: "modifier",
            detail: `Chain through ${t}`,
            sortPriority: 2,
          });
        }
      }
    }
    // Prefer the param's own modifier list; fall back to the generic set only
    // when the server supplies none (keeps type-specific modifiers accurate).
    const own = rootDef.modifiers?.map((m) => m.code) ?? [];
    const mods = new Set(own.length > 0 ? own : SEARCH_MODIFIERS);
    for (const m of mods) {
      if (m.toLowerCase().includes(frag)) {
        out.push({
          label: `:${m}`,
          insertText: `${m}=`,
          filterText: m,
          kind: "modifier",
          detail: `${paramName}:${m}`,
          sortPriority: 0,
        });
      }
    }
    return out;
  }

  // ── Chained: `param:Type.middle.…<fragment>` — resolve the type reached so
  //    far, then suggest that type's search params for the final fragment. ──
  let currentType: string | undefined;
  const first = parts[0];
  if (isReference && knownTypes.has(first) && (rootDef.targets ?? []).includes(first)) {
    currentType = first;
  } else {
    return []; // need an explicit target type to chain unambiguously
  }

  for (let i = 1; i < parts.length - 1; i++) {
    const def: RestConsoleSearchParam | undefined = (
      currentType ? byType[currentType] : undefined
    )?.find((p) => p.code === parts[i]);
    if (!def || def.type.toLowerCase() !== "reference") return [];
    const targets: string[] = def.targets ?? [];
    currentType = targets.length === 1 ? targets[0] : undefined;
    if (!currentType) return []; // ambiguous mid-chain — can't suggest further
  }

  const chainParams = currentType ? byType[currentType] : undefined;
  if (!chainParams) return [];
  const prefix = parts.slice(0, -1).join(".");
  const lastFrag = parts[parts.length - 1].toLowerCase();

  return chainParams
    .filter((p) => p.code.toLowerCase().includes(lastFrag))
    .slice(0, 15)
    .map((p) => {
      const ref = p.type.toLowerCase() === "reference";
      const path = `${prefix}.${p.code}`;
      return {
        label: p.code,
        // Span covers the whole modifier fragment, so replace it in full.
        insertText: ref ? path : `${path}=`,
        filterText: path,
        kind: "param" as const,
        detail: `${currentType}.${p.code} (${p.type})`,
        sortPriority: p.is_common ? 1 : 0,
      };
    });
}

function getQueryValueSuggestions(
  context: CursorContext,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const { resourceType, paramName } = context;
  if (!paramName) return [];

  const fragment = context.fragment.toLowerCase();

  // Special param value suggestions
  if (paramName.startsWith("_")) {
    return getSpecialParamValueSuggestions(paramName, fragment, resourceType, metadata);
  }

  return [];
}

function getSpecialParamValueSuggestions(
  paramName: string,
  fragment: string,
  resourceType: string | undefined,
  metadata: QueryInputMetadata
): QuerySuggestion[] {
  const cap = metadata.capabilities;
  const resCap = resourceType
    ? cap?.resources.find((r) => r.resource_type === resourceType)
    : undefined;

  switch (paramName) {
    case "_sort":
      return getSortValueSuggestions(fragment, resCap);
    case "_summary":
      return getStaticValueSuggestions(["true", "false", "count", "text", "data"], fragment);
    case "_total":
      return getStaticValueSuggestions(["none", "estimate", "accurate"], fragment);
    case "_include":
      return getIncludeSuggestions(fragment, resCap, false);
    case "_revinclude":
      return getIncludeSuggestions(fragment, resCap, true);
    default: {
      // Check special_params examples
      const special = cap?.special_params.find((sp) => sp.name === paramName);
      if (special?.examples.length) {
        return getStaticValueSuggestions(special.examples, fragment);
      }
      return [];
    }
  }
}

function getSortValueSuggestions(
  fragment: string,
  resCap: ResourceCapability | undefined
): QuerySuggestion[] {
  if (!resCap) return [];
  const results: QuerySuggestion[] = [];
  for (const param of resCap.sort_params) {
    if (param.toLowerCase().includes(fragment)) {
      results.push({
        label: param,
        insertText: param,
        kind: "value",
        detail: "Sort ascending",
        sortPriority: 0,
      });
      results.push({
        label: `-${param}`,
        insertText: `-${param}`,
        kind: "value",
        detail: "Sort descending",
        sortPriority: 1,
      });
    }
  }
  return results;
}

function getStaticValueSuggestions(values: string[], fragment: string): QuerySuggestion[] {
  return values
    .filter((v) => v.toLowerCase().includes(fragment))
    .map((v, i) => ({
      label: v,
      insertText: v,
      kind: "value" as const,
      sortPriority: i,
    }));
}

function getIncludeSuggestions(
  fragment: string,
  resCap: ResourceCapability | undefined,
  isRevInclude: boolean
): QuerySuggestion[] {
  if (!resCap) return [];
  const source = isRevInclude ? resCap.rev_includes : resCap.includes;

  const results: QuerySuggestion[] = [];
  for (const inc of source) {
    for (const target of inc.target_types) {
      const label = isRevInclude
        ? inc.param_code
        : `${resCap.resource_type}:${inc.param_code}:${target}`;
      if (label.toLowerCase().includes(fragment)) {
        results.push({
          label,
          insertText: label,
          kind: "value",
          detail: isRevInclude
            ? `Reverse include from ${inc.param_code}`
            : `Include ${target} via ${inc.param_code}`,
          sortPriority: 0,
        });
      }
    }
  }
  return results;
}
