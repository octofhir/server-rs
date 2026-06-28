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

// Brand accent (teal/emerald, hue 168) reused for cursor + active matches.
const ACCENT_DARK = "34d399";
const ACCENT_LIGHT = "059669";

const DARK_RULES: monaco.editor.ITokenThemeRule[] = [
  // Resource / complex types (Patient, Bundle, HumanName) — violet, bold
  { token: "type", foreground: "c4b5fd", fontStyle: "bold" },
  // Function calls (where, ofType, exists) — emerald (brand)
  { token: "predefined", foreground: ACCENT_DARK },
  // Properties (name, given, value) — blue
  { token: "identifier", foreground: "93c5fd" },
  // Operators/keywords (and, or, is, as) — pink
  { token: "keyword", foreground: "f9a8d4" },
  { token: "keyword.special", foreground: "7dd3fc", fontStyle: "italic" },
  { token: "constant", foreground: "7dd3fc" },
  { token: "number", foreground: "67e8f9" },
  { token: "string", foreground: "fcd34d" },
  { token: "operator", foreground: "94a3b8" },
  { token: "comment", foreground: "5a6b78", fontStyle: "italic" },
];

const LIGHT_RULES: monaco.editor.ITokenThemeRule[] = [
  // Resource / complex types — violet, bold
  { token: "type", foreground: "7c3aed", fontStyle: "bold" },
  // Function calls — emerald (brand)
  { token: "predefined", foreground: ACCENT_LIGHT },
  // Properties — blue
  { token: "identifier", foreground: "2563eb" },
  // Operators/keywords — magenta
  { token: "keyword", foreground: "be185d" },
  { token: "keyword.special", foreground: "0369a1", fontStyle: "italic" },
  { token: "constant", foreground: "0369a1" },
  { token: "number", foreground: "0891b2" },
  { token: "string", foreground: "b45309" },
  { token: "operator", foreground: "64748b" },
  { token: "comment", foreground: "8b94a3", fontStyle: "italic" },
];

// Chrome colours — mirror tokens.ts scheme.dark (surfaces are already hex there).
const DARK_COLORS: monaco.editor.IColors = {
  "editor.background": "#06121a", // --octo-surface-1
  "editor.foreground": "#f8f9fe", // --octo-text-primary
  "editorGutter.background": "#06121a",
  "editorLineNumber.foreground": "#5a6b78", // --octo-text-muted
  "editorLineNumber.activeForeground": "#f8f9fe",
  "editor.lineHighlightBackground": "#0a1721", // --octo-surface-2
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": `#${ACCENT_DARK}`,
  "editor.selectionBackground": "#114036", // --octo-accent-primary-bg-hover
  "editor.inactiveSelectionBackground": "#0d2a24", // --octo-accent-primary-subtle
  "editor.selectionHighlightBackground": "#0d2a24",
  "editorWidget.background": "#0a1721",
  "editorWidget.border": "#1d3344", // --octo-border-subtle
  "editorSuggestWidget.background": "#0a1721",
  "editorSuggestWidget.border": "#1d3344",
  "editorSuggestWidget.foreground": "#f8f9fe",
  "editorSuggestWidget.selectedBackground": "#114036",
  "editorHoverWidget.background": "#0a1721",
  "editorHoverWidget.border": "#1d3344",
  "input.background": "#06121a",
  "input.border": "#1d3344",
  "dropdown.background": "#0a1721",
  "dropdown.border": "#1d3344",
  "editorIndentGuide.background": "#1d3344",
  "editorIndentGuide.activeBackground": "#2b5068", // --octo-border-strong
  "scrollbarSlider.background": "#1d334488",
  "scrollbarSlider.hoverBackground": "#2b5068aa",
  "scrollbarSlider.activeBackground": "#2b5068cc",
};

// Chrome colours — mirror tokens.ts scheme.light (oklch surfaces resolved to hex).
const LIGHT_COLORS: monaco.editor.IColors = {
  "editor.background": "#fcfcfd", // --octo-surface-1
  "editor.foreground": "#1b1e26", // --octo-text-primary
  "editorGutter.background": "#fcfcfd",
  "editorLineNumber.foreground": "#9aa1ad", // --octo-text-muted
  "editorLineNumber.activeForeground": "#1b1e26",
  "editor.lineHighlightBackground": "#f5f6f9", // --octo-surface-2
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": `#${ACCENT_LIGHT}`,
  "editor.selectionBackground": "#cdeede", // --octo-accent-primary-subtle (light)
  "editor.inactiveSelectionBackground": "#e6f6ef",
  "editor.selectionHighlightBackground": "#e6f6ef",
  "editorWidget.background": "#f5f6f9",
  "editorWidget.border": "#e6e8ec", // --octo-border-subtle
  "editorSuggestWidget.background": "#ffffff",
  "editorSuggestWidget.border": "#e6e8ec",
  "editorSuggestWidget.foreground": "#1b1e26",
  "editorSuggestWidget.selectedBackground": "#cdeede",
  "editorHoverWidget.background": "#ffffff",
  "editorHoverWidget.border": "#e6e8ec",
  "input.background": "#ffffff",
  "input.border": "#e6e8ec",
  "dropdown.background": "#ffffff",
  "dropdown.border": "#e6e8ec",
  "editorIndentGuide.background": "#e6e8ec",
  "editorIndentGuide.activeBackground": "#d4d8df", // --octo-border-strong
  "scrollbarSlider.background": "#c4cad388",
  "scrollbarSlider.hoverBackground": "#aab1bcaa",
  "scrollbarSlider.activeBackground": "#9aa1adcc",
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
