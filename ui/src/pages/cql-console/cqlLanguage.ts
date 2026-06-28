/**
 * Lightweight Monaco language registration for CQL (Clinical Quality Language).
 *
 * Provides syntax highlighting + bracket/comment config so the console editor
 * reads like real CQL. Tokens map onto the shared octofhir Monaco theme rules
 * (keyword / predefined / type / string / number / comment).
 */

import { monaco } from "@/shared/monaco/config";

export const CQL_LANGUAGE_ID = "cql";

let registered = false;

export function ensureCqlLanguageRegistered(): void {
  if (registered) return;
  registered = true;

  const langs = monaco.languages.getLanguages();
  if (!langs.some((l) => l.id === CQL_LANGUAGE_ID)) {
    monaco.languages.register({ id: CQL_LANGUAGE_ID });
  }

  monaco.languages.setLanguageConfiguration(CQL_LANGUAGE_ID, {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: "'", close: "'" },
      { open: '"', close: '"' },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: "'", close: "'" },
      { open: '"', close: '"' },
    ],
  });

  monaco.languages.setMonarchTokensProvider(CQL_LANGUAGE_ID, {
    ignoreCase: false,
    // Structural / declaration keywords.
    keywords: [
      "library",
      "using",
      "include",
      "version",
      "called",
      "public",
      "private",
      "define",
      "function",
      "context",
      "parameter",
      "default",
      "valueset",
      "codesystem",
      "code",
      "concept",
      "fluent",
      "return",
      "where",
      "with",
      "without",
      "such",
      "that",
      "from",
      "let",
      "aggregate",
      "sort",
      "by",
      "asc",
      "ascending",
      "desc",
      "descending",
      "if",
      "then",
      "else",
      "case",
      "when",
      "end",
    ],
    // Operators / logical words that read as keywords in CQL.
    operatorWords: [
      "and",
      "or",
      "xor",
      "not",
      "implies",
      "is",
      "as",
      "cast",
      "in",
      "contains",
      "properly",
      "between",
      "during",
      "includes",
      "overlaps",
      "before",
      "after",
      "meets",
      "starts",
      "ends",
      "within",
      "same",
      "year",
      "years",
      "month",
      "months",
      "week",
      "weeks",
      "day",
      "days",
      "hour",
      "hours",
      "minute",
      "minutes",
      "second",
      "seconds",
      "millisecond",
      "milliseconds",
      "of",
      "exists",
      "union",
      "intersect",
      "except",
      "distinct",
      "flatten",
      "expand",
      "collapse",
      "singleton",
    ],
    constants: ["true", "false", "null"],
    // Builtin/predefined functions get the brand accent.
    builtins: [
      "Count",
      "Sum",
      "Avg",
      "Min",
      "Max",
      "Median",
      "Mode",
      "StdDev",
      "Variance",
      "First",
      "Last",
      "IndexOf",
      "Length",
      "Coalesce",
      "Combine",
      "Split",
      "Substring",
      "Upper",
      "Lower",
      "ToString",
      "ToInteger",
      "ToDecimal",
      "ToQuantity",
      "ToDate",
      "ToDateTime",
      "Today",
      "Now",
      "TimeOfDay",
      "AgeInYears",
      "AgeInYearsAt",
      "CalculateAgeInYears",
      "Interval",
      "Tuple",
      "List",
    ],
    tokenizer: {
      root: [
        [/\/\/.*$/, "comment"],
        [/\/\*/, "comment", "@comment"],
        // Date / time literals: @2024-01-01, @2024-01-01T10:00, @T10:00
        [/@[0-9T:.\-+Z]+/, "number"],
        // Quoted identifiers "Foo Bar" (CQL allows spaces in define names)
        [/"[^"]*"/, "type"],
        [/'[^']*'/, "string"],
        [
          /[A-Za-z_][\w]*/,
          {
            cases: {
              "@keywords": "keyword",
              "@operatorWords": "keyword",
              "@constants": "constant",
              "@builtins": "predefined",
              "@default": "identifier",
            },
          },
        ],
        [/[0-9]+\.[0-9]+/, "number"],
        [/[0-9]+/, "number"],
        [/[{}()[\]]/, "@brackets"],
        [/[<>=!]+|[-+*/]/, "operator"],
      ],
      comment: [
        [/[^/*]+/, "comment"],
        [/\*\//, "comment", "@pop"],
        [/[/*]/, "comment"],
      ],
    },
  });
}
