import { tokens, type OctoTokens } from "./tokens";
import { mergeDeep, type DeepPartial, type DeepWiden } from "./utils";

export type OctoThemeTokens = DeepWiden<OctoTokens>;
export type OctoThemeTokenOverrides = DeepPartial<OctoThemeTokens>;

export interface OctoThemeConfig {
    /**
     * Deep token override merged over the built-in OctoFHIR theme.
     * Use this for product-level branding while keeping the same token shape.
     */
    tokens?: OctoThemeTokenOverrides;
    /**
     * Escape hatch for CSS variables that do not map to token keys yet.
     * Keys can include the leading `--`.
     */
    cssVariables?: Record<string, string | number>;
}

export type OctoThemeInput = OctoThemeConfig | OctoThemeTokenOverrides;

export const defaultOctoTheme: OctoThemeTokens = tokens;

export function createOctoTheme(theme?: OctoThemeInput): OctoThemeTokens {
    return mergeDeep<OctoThemeTokens>(defaultOctoTheme, normalizeThemeInput(theme).tokens);
}

export function getThemeCssVariables(theme?: OctoThemeInput): Record<string, string> {
    const rawVars = normalizeThemeInput(theme).cssVariables ?? {};
    const vars: Record<string, string> = {};

    for (const [key, value] of Object.entries(rawVars)) {
        vars[key.startsWith("--") ? key : `--${key}`] = String(value);
    }

    return vars;
}

function normalizeThemeInput(theme?: OctoThemeInput): OctoThemeConfig {
    if (!theme) return {};
    if ("tokens" in theme || "cssVariables" in theme) {
        return theme as OctoThemeConfig;
    }
    return { tokens: theme as OctoThemeTokenOverrides };
}
