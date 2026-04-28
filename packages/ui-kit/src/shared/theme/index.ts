import { palette } from "./colors";
import { tokens, type OctoTokens } from "./tokens";

export type OctoThemeOther = OctoTokens;

export { palette, tokens };
export {
    ColorSchemeProvider,
    useColorScheme,
    type ColorScheme,
    type ColorSchemePreference,
    type ColorSchemeContextValue,
    type ColorSchemeProviderProps,
} from "./color-scheme";
