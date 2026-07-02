import type { RestConsoleSearchParam } from "@/shared/api";
import { fullKey, parseSearchKey, SEARCH_MODIFIERS } from "./search-key";
import type { Diagnostic, QueryAst, QueryInputMetadata, QueryParamNode } from "./types";

const VALID_SUMMARY_VALUES = new Set(["true", "false", "count", "text", "data"]);
const VALID_TOTAL_VALUES = new Set(["none", "estimate", "accurate"]);
const PREFIX_TYPES = new Set(["date", "number", "quantity"]);

export function computeDiagnostics(ast: QueryAst, metadata: QueryInputMetadata): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];

  // Path-level diagnostics
  checkPath(ast, metadata, diagnostics);

  // Query param diagnostics
  checkParams(ast, metadata, diagnostics);

  return diagnostics;
}

function checkPath(ast: QueryAst, metadata: QueryInputMetadata, diagnostics: Diagnostic[]): void {
  const { path } = ast;

  if (
    path.kind === "resource-type" ||
    path.kind === "resource-instance" ||
    path.kind === "type-operation" ||
    path.kind === "instance-operation"
  ) {
    const rt = path.resourceType;
    if (rt && metadata.resourceTypes.length > 0 && !metadata.resourceTypes.includes(rt)) {
      diagnostics.push({
        severity: "error",
        message: `Unknown resource type '${rt}'`,
        span: path.span,
        code: "unknown-resource",
      });
    }
  }
}

function checkParams(ast: QueryAst, metadata: QueryInputMetadata, diagnostics: Diagnostic[]): void {
  const resourceType = getResourceType(ast);
  const seen = new Map<string, QueryParamNode>();

  for (const param of ast.params) {
    // Empty param name
    if (!param.name) {
      diagnostics.push({
        severity: "error",
        message: "Empty parameter name",
        span: param.span,
        code: "empty-param-name",
      });
      continue;
    }

    // Duplicate param detection (skip special params that can repeat like _include)
    const repeatableParams = new Set(["_include", "_revinclude", "_has", "_sort"]);
    if (!repeatableParams.has(param.name)) {
      const prev = seen.get(param.name);
      if (prev) {
        diagnostics.push({
          severity: "warning",
          message: `Duplicate parameter '${param.name}'`,
          span: param.span,
          code: "duplicate-param",
        });
      }
    }
    seen.set(param.name, param);

    // Special params validation
    if (param.isSpecial) {
      checkSpecialParam(param, resourceType, metadata, diagnostics);
      continue;
    }

    // Regular param validation (requires resource type and metadata)
    if (resourceType) {
      checkRegularParam(param, resourceType, metadata, diagnostics);
    }
  }
}

function checkSpecialParam(
  param: QueryParamNode,
  resourceType: string | undefined,
  metadata: QueryInputMetadata,
  diagnostics: Diagnostic[]
): void {
  switch (param.name) {
    case "_count": {
      for (const v of param.values) {
        if (v.raw && (!/^\d+$/.test(v.raw) || Number.parseInt(v.raw) <= 0)) {
          diagnostics.push({
            severity: "error",
            message: `_count must be a positive integer, got '${v.raw}'`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
    case "_offset": {
      for (const v of param.values) {
        if (v.raw && !/^\d+$/.test(v.raw)) {
          diagnostics.push({
            severity: "error",
            message: `_offset must be a non-negative integer, got '${v.raw}'`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
    case "_summary": {
      for (const v of param.values) {
        if (v.raw && !VALID_SUMMARY_VALUES.has(v.raw)) {
          diagnostics.push({
            severity: "error",
            message: `Invalid _summary value '${v.raw}'. Expected: true, false, count, text, data`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
    case "_total": {
      for (const v of param.values) {
        if (v.raw && !VALID_TOTAL_VALUES.has(v.raw)) {
          diagnostics.push({
            severity: "error",
            message: `Invalid _total value '${v.raw}'. Expected: none, estimate, accurate`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
    case "_sort": {
      if (!resourceType || !metadata.capabilities) break;
      const resCap = metadata.capabilities.resources.find((r) => r.resource_type === resourceType);
      if (!resCap) break;
      for (const v of param.values) {
        const sortField = v.raw.startsWith("-") ? v.raw.slice(1) : v.raw;
        if (sortField && !resCap.sort_params.includes(sortField)) {
          diagnostics.push({
            severity: "warning",
            message: `Unknown sort parameter '${sortField}' for ${resourceType}`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
    case "_include":
    case "_revinclude": {
      if (!resourceType || !metadata.capabilities) break;
      const isRev = param.name === "_revinclude";
      // `:iterate` / `:recurse` includes may pull from a type already in the
      // bundle, so the source type need not be the search root.
      const isIterate = param.modifier === "iterate" || param.modifier === "recurse";
      const caps = metadata.capabilities.resources;

      for (const v of param.values) {
        const raw = v.raw;
        if (!raw || raw === "*") continue;

        // Value grammar: SourceType:searchParam[:TargetType]
        const parts = raw.split(":");
        if (parts.length < 2 || !parts[0] || !parts[1]) {
          diagnostics.push({
            severity: "warning",
            message: `Malformed ${param.name} value '${raw}'. Expected SourceType:searchParam[:TargetType]`,
            span: v.span,
            code: "invalid-value",
          });
          continue;
        }
        const [srcType, paramCode] = parts;

        // Which resource's include list governs this value?
        const capType = isIterate ? srcType : resourceType;
        const resCap = caps.find((r) => r.resource_type === capType);
        // No capability metadata for the governing type — let the server judge
        // rather than emit a false positive (common for iterate cross-type).
        if (!resCap) continue;

        const list = isRev ? resCap.rev_includes : resCap.includes;
        if (list.length === 0) continue;

        const ok = list.some((inc) =>
          isRev
            ? // rev_include codes may be stored as `Source:param` or plain `param`
              inc.param_code === `${srcType}:${paramCode}` || inc.param_code === paramCode
            : inc.param_code === paramCode && srcType === capType
        );
        if (!ok) {
          diagnostics.push({
            severity: "warning",
            message: `Unknown ${param.name} value '${raw}' for ${capType}`,
            span: v.span,
            code: "invalid-value",
          });
        }
      }
      break;
    }
  }
}

function checkRegularParam(
  param: QueryParamNode,
  resourceType: string,
  metadata: QueryInputMetadata,
  diagnostics: Diagnostic[]
): void {
  // Nothing to validate against — bail without flagging.
  if (!metadata.searchParamsByResource[resourceType]) return;

  const knownTypes = new Set(metadata.resourceTypes);
  const parsed = parseSearchKey(fullKey(param.name, param.modifier), knownTypes);

  // Reverse chains (`_has:…`) are routed through checkSpecialParam; a stray
  // one here is left to the server to validate rather than false-flagged.
  if (parsed.isHas) return;

  // Walk the chain link by link, resolving the target resource type at each
  // reference hop so deeper links validate against the right type.
  let currentType: string | undefined = resourceType;
  for (let i = 0; i < parsed.segments.length; i++) {
    const seg = parsed.segments[i];
    const isLast = i === parsed.segments.length - 1;

    const params: RestConsoleSearchParam[] | undefined = currentType
      ? metadata.searchParamsByResource[currentType]
      : undefined;
    // No metadata for this hop's type — can't validate deeper without
    // guessing, so stop silently instead of emitting a false error.
    if (!params) return;

    const def: RestConsoleSearchParam | undefined = params.find((p) => p.code === seg.code);
    if (!def) {
      diagnostics.push({
        severity: "error",
        message:
          i === 0
            ? `Unknown search parameter '${seg.code}' for ${currentType}`
            : `Unknown chained parameter '${seg.code}' for ${currentType}`,
        span: param.span,
        code: "unknown-param",
      });
      return;
    }

    if (!isLast) {
      // Intermediate link must be a reference to chain further.
      if (def.type.toLowerCase() !== "reference") {
        diagnostics.push({
          severity: "warning",
          message: `Cannot chain through '${seg.code}' — it is a ${def.type} parameter, not a reference`,
          span: param.span,
          code: "invalid-modifier",
        });
        return;
      }
      const targets: string[] = def.targets ?? [];
      if (seg.typeModifier) {
        if (knownTypes.size > 0 && !knownTypes.has(seg.typeModifier)) {
          diagnostics.push({
            severity: "warning",
            message: `Unknown type modifier ':${seg.typeModifier}'`,
            span: param.span,
            code: "invalid-modifier",
          });
          return;
        }
        if (targets.length > 0 && !targets.includes(seg.typeModifier)) {
          diagnostics.push({
            severity: "warning",
            message: `'${seg.typeModifier}' is not a target of '${seg.code}' (targets: ${targets.join(", ")})`,
            span: param.span,
            code: "invalid-modifier",
          });
          return;
        }
        currentType = seg.typeModifier;
      } else if (targets.length === 1) {
        currentType = targets[0];
      } else {
        // Untyped chain through a multi-target reference — legal syntax but
        // the server needs a :Type to disambiguate. Warn, don't hard-fail.
        const hint = targets[0] ?? "Type";
        diagnostics.push({
          severity: "warning",
          message: `Ambiguous chain: '${seg.code}' has multiple targets${targets.length ? ` (${targets.join(", ")})` : ""}. Add a type modifier, e.g. '${seg.code}:${hint}.…'`,
          span: param.span,
          code: "invalid-modifier",
        });
        return;
      }
      continue;
    }

    // Terminal link ----------------------------------------------------
    // A type modifier on a terminal reference (`subject:Patient`) constrains
    // the target type — validate it against the param's targets.
    if (seg.typeModifier) {
      if (knownTypes.size > 0 && !knownTypes.has(seg.typeModifier)) {
        diagnostics.push({
          severity: "warning",
          message: `Unknown type modifier ':${seg.typeModifier}'`,
          span: param.span,
          code: "invalid-modifier",
        });
      } else if (def.type.toLowerCase() === "reference") {
        const targets: string[] = def.targets ?? [];
        if (targets.length > 0 && !targets.includes(seg.typeModifier)) {
          diagnostics.push({
            severity: "warning",
            message: `'${seg.typeModifier}' is not a target of '${seg.code}' (targets: ${targets.join(", ")})`,
            span: param.span,
            code: "invalid-modifier",
          });
        }
      } else {
        diagnostics.push({
          severity: "warning",
          message: `Type modifier ':${seg.typeModifier}' only applies to reference parameters, not '${seg.code}'`,
          span: param.span,
          code: "invalid-modifier",
        });
      }
    }

    // Trailing search modifier (`:exact`, `:missing`, …). Modifiers are
    // type-specific, so validate against the param's own allowed list; only
    // fall back to the generic set when the server supplies none.
    if (parsed.terminalModifier) {
      const allowed = def.modifiers?.map((m) => m.code) ?? [];
      const valid = new Set(allowed.length > 0 ? allowed : SEARCH_MODIFIERS);
      if (!valid.has(parsed.terminalModifier)) {
        diagnostics.push({
          severity: "warning",
          message: `Unsupported modifier ':${parsed.terminalModifier}' for parameter '${seg.code}'`,
          span: param.span,
          code: "invalid-modifier",
        });
      }
    }

    // Prefixes only apply to date, number, quantity params.
    const paramType = def.type.toLowerCase();
    for (const v of param.values) {
      if (v.prefix && !PREFIX_TYPES.has(paramType)) {
        diagnostics.push({
          severity: "error",
          message: `Search prefix '${v.prefix}' is not valid for ${paramType} parameter '${seg.code}'. Prefixes only apply to date, number, quantity types`,
          span: v.span,
          code: "invalid-prefix",
        });
      }
    }
  }
}

function getResourceType(ast: QueryAst): string | undefined {
  const { path } = ast;
  switch (path.kind) {
    case "resource-type":
    case "resource-instance":
    case "type-operation":
    case "instance-operation":
      return path.resourceType;
    default:
      return undefined;
  }
}
