import { useColorScheme } from "../../theme/color-scheme";
import { OCTO_THEME_DARK, OCTO_THEME_LIGHT } from "./echartsTheme";

/** Resolve the registered brand theme name for the active color scheme. */
export function useChartTheme(): typeof OCTO_THEME_LIGHT | typeof OCTO_THEME_DARK {
  const { colorScheme } = useColorScheme();
  return colorScheme === "dark" ? OCTO_THEME_DARK : OCTO_THEME_LIGHT;
}
