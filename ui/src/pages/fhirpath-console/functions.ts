/**
 * Curated FHIRPath function reference used by the console's function palette.
 * Each entry inserts a Monaco snippet at the cursor (placeholders use ${n:..}).
 */

export interface FhirPathFn {
  name: string;
  /** Display signature, e.g. "where(criteria)". */
  signature: string;
  /** Short description shown on hover. */
  doc: string;
  /** Monaco snippet inserted at cursor. */
  snippet: string;
}

export interface FhirPathFnCategory {
  label: string;
  functions: FhirPathFn[];
}

export const FHIRPATH_FUNCTIONS: FhirPathFnCategory[] = [
  {
    label: "Filtering & projection",
    functions: [
      {
        name: "where",
        signature: "where(criteria)",
        doc: "Keep only items matching the boolean criteria.",
        snippet: "where(${1:criteria})",
      },
      {
        name: "select",
        signature: "select(projection)",
        doc: "Project each item to a new value/collection.",
        snippet: "select(${1:projection})",
      },
      {
        name: "ofType",
        signature: "ofType(type)",
        doc: "Filter items to those of the given FHIR type.",
        snippet: "ofType(${1:Patient})",
      },
      {
        name: "repeat",
        signature: "repeat(projection)",
        doc: "Recursively apply the projection until no new items.",
        snippet: "repeat(${1:projection})",
      },
      {
        name: "extension",
        signature: "extension(url)",
        doc: "Shortcut for extensions with the given url.",
        snippet: "extension('${1:url}')",
      },
    ],
  },
  {
    label: "Existence",
    functions: [
      {
        name: "exists",
        signature: "exists([criteria])",
        doc: "True if the collection has any (matching) item.",
        snippet: "exists(${1})",
      },
      {
        name: "empty",
        signature: "empty()",
        doc: "True if the collection is empty.",
        snippet: "empty()",
      },
      {
        name: "all",
        signature: "all(criteria)",
        doc: "True if every item matches the criteria.",
        snippet: "all(${1:criteria})",
      },
      {
        name: "count",
        signature: "count()",
        doc: "Number of items in the collection.",
        snippet: "count()",
      },
      {
        name: "distinct",
        signature: "distinct()",
        doc: "Remove duplicate items.",
        snippet: "distinct()",
      },
    ],
  },
  {
    label: "Subsetting",
    functions: [
      {
        name: "first",
        signature: "first()",
        doc: "First item of the collection.",
        snippet: "first()",
      },
      {
        name: "last",
        signature: "last()",
        doc: "Last item of the collection.",
        snippet: "last()",
      },
      {
        name: "tail",
        signature: "tail()",
        doc: "All items except the first.",
        snippet: "tail()",
      },
      {
        name: "skip",
        signature: "skip(num)",
        doc: "Skip the first num items.",
        snippet: "skip(${1:num})",
      },
      {
        name: "take",
        signature: "take(num)",
        doc: "Take the first num items.",
        snippet: "take(${1:num})",
      },
      {
        name: "single",
        signature: "single()",
        doc: "The sole item, or error if not exactly one.",
        snippet: "single()",
      },
    ],
  },
  {
    label: "Strings",
    functions: [
      {
        name: "substring",
        signature: "substring(start[, length])",
        doc: "Substring from start (0-based).",
        snippet: "substring(${1:0}${2:, length})",
      },
      {
        name: "startsWith",
        signature: "startsWith(prefix)",
        doc: "True if the string starts with prefix.",
        snippet: "startsWith('${1:prefix}')",
      },
      {
        name: "contains",
        signature: "contains(substring)",
        doc: "True if the string contains the substring.",
        snippet: "contains('${1:text}')",
      },
      {
        name: "matches",
        signature: "matches(regex)",
        doc: "True if the string matches the regex.",
        snippet: "matches('${1:regex}')",
      },
      {
        name: "replace",
        signature: "replace(pattern, sub)",
        doc: "Replace occurrences of pattern with sub.",
        snippet: "replace('${1:pattern}', '${2:sub}')",
      },
      {
        name: "upper",
        signature: "upper()",
        doc: "Uppercase the string.",
        snippet: "upper()",
      },
      {
        name: "lower",
        signature: "lower()",
        doc: "Lowercase the string.",
        snippet: "lower()",
      },
    ],
  },
  {
    label: "Logic & conversion",
    functions: [
      {
        name: "iif",
        signature: "iif(cond, then[, else])",
        doc: "Inline conditional expression.",
        snippet: "iif(${1:cond}, ${2:then}, ${3:else})",
      },
      {
        name: "toString",
        signature: "toString()",
        doc: "Convert the value to a string.",
        snippet: "toString()",
      },
      {
        name: "toInteger",
        signature: "toInteger()",
        doc: "Convert the value to an integer.",
        snippet: "toInteger()",
      },
      {
        name: "toQuantity",
        signature: "toQuantity()",
        doc: "Convert the value to a Quantity.",
        snippet: "toQuantity()",
      },
    ],
  },
  {
    label: "Date & utility",
    functions: [
      {
        name: "today",
        signature: "today()",
        doc: "Current date.",
        snippet: "today()",
      },
      {
        name: "now",
        signature: "now()",
        doc: "Current date and time.",
        snippet: "now()",
      },
      {
        name: "resolve",
        signature: "resolve()",
        doc: "Resolve a Reference to its target resource.",
        snippet: "resolve()",
      },
      {
        name: "trace",
        signature: "trace(name)",
        doc: "Log intermediate values for debugging.",
        snippet: "trace('${1:label}')",
      },
    ],
  },
];
