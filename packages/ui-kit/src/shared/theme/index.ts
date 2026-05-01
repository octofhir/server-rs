import { palette } from "./colors";
import { tokens, type OctoTokens } from "./tokens";

export type OctoThemeOther = OctoTokens;

export { palette, tokens };
export {
    createOctoTheme,
    defaultOctoTheme,
    getThemeCssVariables,
    type OctoThemeConfig,
    type OctoThemeInput,
    type OctoThemeTokenOverrides,
    type OctoThemeTokens,
} from "./theme-config";
export { type DeepPartial, type DeepWiden } from "./utils";
export {
    ColorSchemeProvider,
    useColorScheme,
    type ColorScheme,
    type ColorSchemePreference,
    type ColorSchemeContextValue,
    type ColorSchemeProviderProps,
} from "./color-scheme";
