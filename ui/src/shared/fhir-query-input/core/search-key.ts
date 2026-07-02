// FHIR search-parameter KEY grammar (the part left of `=` in a query token).
//
// A key can be:
//   • a simple param code                        → `name`
//   • with a search modifier                      → `name:exact`, `code:missing`
//   • a chain, each link optionally type-scoped   → `patient:Patient.organization.name`
//   • a terminal type modifier on a reference     → `subject:Patient`
//   • a reverse chain                             → `_has:Observation:patient:code`
//
// The old parser split on the FIRST `:` and treated the rest as one opaque
// "modifier", which wrongly flagged valid typed chains like
// `patient:Patient.organization.name`. This module parses the real grammar.

/** Non-type search modifiers defined by the FHIR spec (R4/R5). */
export const SEARCH_MODIFIERS = new Set([
  "missing",
  "exact",
  "contains",
  "text",
  "not",
  "above",
  "below",
  "in",
  "not-in",
  "of-type",
  "ofType",
  "identifier",
  "code-text",
  "text-advanced",
  "iterate",
  "recurse",
]);

/** One link in a chain: a param code, optionally scoped to a target resource type. */
export type ChainSegment = { code: string; typeModifier?: string };

export type ParsedSearchKey = {
  /** Reverse chain `_has:Type:ref:param[:_has:…]` — validated loosely (server owns semantics). */
  isHas: boolean;
  /** Ordered chain links. Single-element for a plain param. Empty for `_has`. */
  segments: ChainSegment[];
  /** Trailing search modifier on the terminal segment (`:exact`, `:missing`, …). */
  terminalModifier?: string;
  /** Raw `_has` key when isHas. */
  hasRaw?: string;
};

/**
 * Parse a full search key into chain segments + terminal modifier.
 * `knownTypes` disambiguates a terminal `:Token` between a type modifier
 * (`subject:Patient`) and a search modifier (`name:exact`).
 */
export function parseSearchKey(full: string, knownTypes: Set<string>): ParsedSearchKey {
  if (full.startsWith("_has:") || full === "_has") {
    return { isHas: true, segments: [], hasRaw: full };
  }

  const parts = full.split(".");
  const segments: ChainSegment[] = [];
  let terminalModifier: string | undefined;

  parts.forEach((part, i) => {
    const isLast = i === parts.length - 1;
    const colon = part.indexOf(":");
    if (colon === -1) {
      segments.push({ code: part });
      return;
    }
    const code = part.slice(0, colon);
    const rest = part.slice(colon + 1);
    if (isLast) {
      // Terminal `:x` — a resource type means a reference type modifier,
      // otherwise it's a search modifier.
      if (knownTypes.has(rest)) {
        segments.push({ code, typeModifier: rest });
      } else {
        terminalModifier = rest;
        segments.push({ code });
      }
    } else {
      // Mid-chain `:x` can only be a reference type modifier.
      segments.push({ code, typeModifier: rest });
    }
  });

  return { isHas: false, segments, terminalModifier };
}

/** Reconstruct the full key from the parser's split name + modifier fields. */
export function fullKey(name: string, modifier?: string): string {
  return modifier === undefined ? name : `${name}:${modifier}`;
}
