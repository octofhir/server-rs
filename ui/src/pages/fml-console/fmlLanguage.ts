/**
 * Monaco language registration for the FHIR Mapping Language (FML).
 *
 * Syntax highlighting and bracket/comment configuration only; problems come from the server
 * (`/api/fml/check`). Tokens map onto the shared octofhir Monaco theme rules.
 */

import { monaco } from "@/shared/monaco/config";

export const FML_LANGUAGE_ID = "fml";

let registered = false;

export function ensureFmlLanguageRegistered(): void {
  if (registered) return;
  registered = true;

  if (!monaco.languages.getLanguages().some((l) => l.id === FML_LANGUAGE_ID)) {
    monaco.languages.register({ id: FML_LANGUAGE_ID, extensions: [".fml", ".map"] });
  }

  monaco.languages.setLanguageConfiguration(FML_LANGUAGE_ID, {
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
      { open: "`", close: "`" },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "(", close: ")" },
      { open: "'", close: "'" },
      { open: '"', close: '"' },
    ],
  });

  monaco.languages.setMonarchTokensProvider(FML_LANGUAGE_ID, {
    keywords: [
      "map",
      "uses",
      "as",
      "alias",
      "imports",
      "group",
      "extends",
      "default",
      "where",
      "check",
      "log",
      "then",
      "source",
      "target",
      "queried",
      "produced",
      "conceptmap",
      "prefix",
      "let",
      "types",
      "type",
      "first",
      "not_first",
      "last",
      "not_last",
      "only_one",
      "share",
      "single",
    ],
    constants: ["true", "false"],
    transforms: [
      "create",
      "copy",
      "truncate",
      "escape",
      "cast",
      "append",
      "translate",
      "reference",
      "pointer",
      "uuid",
      "evaluate",
      "cc",
      "c",
      "qty",
      "id",
      "cp",
      "toDateTime",
      "toDate",
      "toTime",
      "unixToDateTime",
      "unixToDate",
      "unixToTime",
    ],
    tokenizer: {
      root: [
        // `/// key = value` metadata
        [/\/\/\/(?!\/)/, "keyword", "@metadata"],
        [/\/\/.*$/, "comment"],
        [/\/\*/, "comment", "@comment"],
        [/<<\s*(types|type\+)\s*>>/, "keyword"],
        [/"""/, "string", "@markdown"],
        [/'([^'\\]|\\.)*'/, "string"],
        [/"([^"\\]|\\.)*"/, "string"],
        [/`[^`]*`/, "identifier"],
        [/@[0-9T:.\-+Z]+/, "number"],
        [/[0-9]+(\.[0-9]+)?/, "number"],
        [/->/, "keyword"],
        [
          /[A-Za-z_][\w]*(?=\s*\()/,
          { cases: { "@transforms": "predefined", "@keywords": "keyword", "@default": "identifier" } },
        ],
        [
          /[A-Za-z_][\w]*/,
          { cases: { "@keywords": "keyword", "@constants": "constant", "@default": "identifier" } },
        ],
        [/[{}()[\]]/, "@brackets"],
      ],
      metadata: [
        [/[A-Za-z_][\w.]*/, "type"],
        [/$/, "", "@pop"],
        [/'([^'\\]|\\.)*'/, "string"],
        [/"""/, "string", "@markdown"],
        [/./, ""],
      ],
      markdown: [
        [/"""/, "string", "@pop"],
        [/./, "string"],
      ],
      comment: [
        [/\*\//, "comment", "@pop"],
        [/./, "comment"],
      ],
    },
  });
}
