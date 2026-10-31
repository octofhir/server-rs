/**
 * Shared Monaco editor theme.
 *
 * One light + one dark theme used by every Monaco editor in the console
 * (SQL, FHIRPath, JSON, CQL, automation/policy scripts, FHIR query input).
 *
 * The themes superset the stock `vs` / `vs-dark`:
 * - `colors` map the editor chrome (background, gutter, line-highlight,
 *   selection, suggest widget…) onto the app's `--octo-*` design tokens so the
 *   editor surface matches the surrounding UI in both schemes. These mirror
 *   `packages/ui-kit/src/shared/theme/tokens.ts` (`scheme.light` / `scheme.dark`).
 *   Monaco needs concrete hex (no `oklch`/CSS vars), so the surface values are
 *   resolved here — keep them in sync with tokens.ts.
 * - `rules` colour the language-agnostic Monarch token types so FHIRPath / SQL /
 *   JS highlight consistently.
 */

import { useColorScheme } from "@octofhir/ui-kit";
import { monaco } from "./config";

export const OCTOFHIR_THEME_DARK = "octofhir-dark";
export const OCTOFHIR_THEME_LIGHT = "octofhir-light";

// Marigold accent: cursor, function names and selection. Everything else stays restrained
// (blue keywords, green strings, cyan numbers, muted operators) so code never looks like a rainbow.
const ACCENT_DARK = "f5bd3b";
const ACCENT_TEXT_DARK = "f7c95a";
const ACCENT_TEXT_LIGHT = "855700";

const DARK_RULES: monaco.editor.ITokenThemeRule[] = [
  { token: "type", foreground: "ededef", fontStyle: "bold" },
  { token: "predefined", foreground: ACCENT_TEXT_DARK },
  { token: "identifier", foreground: "c9cad0" },
  { token: "keyword", foreground: "8fb3ff" },
  { token: "keyword.special", foreground: "8fb3ff", fontStyle: "italic" },
  { token: "constant", foreground: "8fb3ff" },
  { token: "number", foreground: "6fd3e0" },
  { token: "string", foreground: "86d39d" },
  { token: "operator", foreground: "8b8b94" },
  { token: "comment", foreground: "6b6b74", fontStyle: "italic" },
  // The vs-dark base ships SQL-specific rules (red strings, magenta builtins) that would win
  // over the generic ones above.
  { token: "string.sql", foreground: "86d39d" },
  { token: "predefined.sql", foreground: ACCENT_TEXT_DARK },
  { token: "operator.sql", foreground: "8b8b94" },
];

const LIGHT_RULES: monaco.editor.ITokenThemeRule[] = [
  { token: "type", foreground: "18181b", fontStyle: "bold" },
  { token: "predefined", foreground: ACCENT_TEXT_LIGHT },
  { token: "identifier", foreground: "3f3f46" },
  { token: "keyword", foreground: "1d4ed8" },
  { token: "keyword.special", foreground: "1d4ed8", fontStyle: "italic" },
  { token: "constant", foreground: "1d4ed8" },
  { token: "number", foreground: "0e7490" },
  { token: "string", foreground: "15803d" },
  { token: "operator", foreground: "6b6b74" },
  { token: "comment", foreground: "8b8b94", fontStyle: "italic" },
  { token: "string.sql", foreground: "15803d" },
  { token: "predefined.sql", foreground: ACCENT_TEXT_LIGHT },
  { token: "operator.sql", foreground: "6b6b74" },
];

// Chrome colours — mirror tokens.ts scheme.dark.
const DARK_COLORS: monaco.editor.IColors = {
  "editor.background": "#111214", // --octo-surface-1
  "editor.foreground": "#ededef", // --octo-text-primary
  "editorGutter.background": "#111214",
  "editorLineNumber.foreground": "#6b6b74",
  "editorLineNumber.activeForeground": "#ededef",
  "editor.lineHighlightBackground": "#18191c", // --octo-surface-2
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": `#${ACCENT_DARK}`,
  "editor.selectionBackground": "#3a2d10", // --octo-accent-primary-bg-hover
  "editor.inactiveSelectionBackground": "#2b220d", // --octo-accent-primary-bg
  "editor.selectionHighlightBackground": "#2b220d",
  "editorWidget.background": "#18191c",
  "editorWidget.border": "#26272b", // --octo-border-subtle
  "editorSuggestWidget.background": "#18191c",
  "editorSuggestWidget.border": "#26272b",
  "editorSuggestWidget.foreground": "#ededef",
  "editorSuggestWidget.selectedBackground": "#2b220d",
  "editorSuggestWidget.selectedForeground": "#ededef",
  "editorSuggestWidget.selectedIconForeground": "#ededef",
  "editorSuggestWidget.highlightForeground": "#f7c95a", // --octo-accent-primary-text
  "editorSuggestWidget.focusHighlightForeground": "#f7c95a",
  "list.activeSelectionForeground": "#ededef",
  "editorHoverWidget.background": "#18191c",
  "editorHoverWidget.border": "#26272b",
  "input.background": "#111214",
  "input.border": "#26272b",
  "dropdown.background": "#18191c",
  "dropdown.border": "#26272b",
  "editorIndentGuide.background": "#26272b",
  "editorIndentGuide.activeBackground": "#34353a", // --octo-border-strong
  "scrollbarSlider.background": "#34353a88",
  "scrollbarSlider.hoverBackground": "#4a4b52aa",
  "scrollbarSlider.activeBackground": "#5a5b63cc",
  // Kill Monaco's default drop shadows (widgets + scroll edge) for a flat look.
  "widget.shadow": "#00000000",
  "scrollbar.shadow": "#00000000",
};

// Chrome colours — mirror tokens.ts scheme.light.
const LIGHT_COLORS: monaco.editor.IColors = {
  "editor.background": "#ffffff", // --octo-surface-1
  "editor.foreground": "#18181b", // --octo-text-primary
  "editorGutter.background": "#ffffff",
  "editorLineNumber.foreground": "#a1a1aa",
  "editorLineNumber.activeForeground": "#18181b",
  "editor.lineHighlightBackground": "#f4f4f5", // --octo-surface-2
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": "#b98309", // --octo-focus-ring
  "editor.selectionBackground": "#fbe9b8", // --octo-accent-primary-bg-hover
  "editor.inactiveSelectionBackground": "#fdf3d7", // --octo-accent-primary-bg
  "editor.selectionHighlightBackground": "#fdf3d7",
  "editorWidget.background": "#ffffff",
  "editorWidget.border": "#e4e4e7", // --octo-border-subtle
  "editorSuggestWidget.background": "#ffffff",
  "editorSuggestWidget.border": "#e4e4e7",
  "editorSuggestWidget.foreground": "#18181b",
  "editorSuggestWidget.selectedBackground": "#fdf3d7",
  // The base light theme draws the focused row's text white; keep it readable on the pale fill.
  "editorSuggestWidget.selectedForeground": "#18181b",
  "editorSuggestWidget.selectedIconForeground": "#18181b",
  "editorSuggestWidget.highlightForeground": "#855700", // --octo-accent-primary-text
  "editorSuggestWidget.focusHighlightForeground": "#855700",
  "list.activeSelectionForeground": "#18181b",
  "editorHoverWidget.background": "#ffffff",
  "editorHoverWidget.border": "#e4e4e7",
  "input.background": "#ffffff",
  "input.border": "#e4e4e7",
  "dropdown.background": "#ffffff",
  "dropdown.border": "#e4e4e7",
  "editorIndentGuide.background": "#e4e4e7",
  "editorIndentGuide.activeBackground": "#d4d4d8", // --octo-border-strong
  "scrollbarSlider.background": "#d4d4d888",
  "scrollbarSlider.hoverBackground": "#a1a1aaaa",
  "scrollbarSlider.activeBackground": "#8b8b94cc",
  // Kill Monaco's default drop shadows (widgets + scroll edge) for a flat look.
  "widget.shadow": "#00000000",
  "scrollbar.shadow": "#00000000",
};

let themesDefined = false;

/** Define the octofhir editor themes once. Idempotent. */
export function ensureOctofhirThemes(): void {
  if (themesDefined || typeof window === "undefined") {
    return;
  }
  try {
    monaco.editor.defineTheme(OCTOFHIR_THEME_DARK, {
      base: "vs-dark",
      inherit: true,
      rules: DARK_RULES,
      colors: DARK_COLORS,
    });
    monaco.editor.defineTheme(OCTOFHIR_THEME_LIGHT, {
      base: "vs",
      inherit: true,
      rules: LIGHT_RULES,
      colors: LIGHT_COLORS,
    });
    themesDefined = true;
  } catch {
    /* monaco not ready; will retry on next call */
  }
}

// Define eagerly on import so the editor's `theme` prop resolves on mount.
ensureOctofhirThemes();

/**
 * Resolve the octofhir Monaco theme name for the active color scheme.
 * Ensures the themes are registered, then returns the matching name.
 */
export function useOctoMonacoTheme(): string {
  ensureOctofhirThemes();
  const { colorScheme } = useColorScheme();
  return colorScheme === "dark" ? OCTOFHIR_THEME_DARK : OCTOFHIR_THEME_LIGHT;
}
