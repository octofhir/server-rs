/**
 * OctoFHIR ECharts theme. Builds a theme object straight from the brand
 * `palette` + design `tokens` so every chart renders with the emerald-led
 * brand palette instead of stock ECharts colors. Registers `octo-light` and
 * `octo-dark` at module load.
 */
import type { ColorScheme } from "../../theme/color-scheme";
import { palette } from "../../theme/colors";
import { tokens } from "../../theme/tokens";
import { echarts } from "./echarts";

export const OCTO_THEME_LIGHT = "octo-light";
export const OCTO_THEME_DARK = "octo-dark";

/** Add an alpha channel to an oklch/rgb color string. */
function withAlpha(color: string, a: number): string {
  return color.replace(/\)\s*$/, ` / ${a})`);
}

/** Emerald-led categorical series color order. */
function categoricalColors(): string[] {
  return [
    palette.primary[5],
    palette.accent[5],
    palette.info[5],
    palette.warning[5],
    palette.error[5],
    palette.success[5],
    palette.deep[6],
    palette.primary[7],
    palette.accent[7],
    palette.info[7],
  ];
}

/** Build an ECharts theme object for the given color scheme from brand tokens. */
export function buildOctoTheme(scheme: ColorScheme): object {
  const s = tokens.scheme[scheme];
  const textPrimary = s.text.primary;
  const textSecondary = s.text.secondary;
  const borderSubtle = s.border.subtle;
  const surface1 = s.surface[1];
  const pointer = palette.primary[5];
  const pointerShadow = withAlpha(palette.primary[5], 0.12);
  // Keep the original series color on hover (ECharts' default brightening
  // washes mid-tones to near-white on the light scheme).
  const keepColor = { emphasis: { itemStyle: { color: "inherit" } } };

  const axisCommon = {
    axisLine: { lineStyle: { color: borderSubtle } },
    axisTick: { lineStyle: { color: borderSubtle } },
    axisLabel: { color: textSecondary },
    splitLine: { lineStyle: { color: borderSubtle } },
    splitArea: { areaStyle: { color: ["transparent", "transparent"] } },
  };

  return {
    color: categoricalColors(),
    backgroundColor: "transparent",
    textStyle: { color: textPrimary },
    title: {
      textStyle: { color: textSecondary },
      subtextStyle: { color: s.text.muted },
    },
    legend: {
      textStyle: { color: textSecondary },
    },
    categoryAxis: axisCommon,
    valueAxis: axisCommon,
    logAxis: axisCommon,
    timeAxis: axisCommon,
    line: { symbol: "circle", smooth: false, emphasis: { lineStyle: { width: 3 } } },
    bar: keepColor,
    scatter: keepColor,
    pie: {
      // Separator stroke that reads on both schemes; keep hue on hover.
      itemStyle: { borderColor: surface1, borderWidth: 1 },
      emphasis: { itemStyle: { color: "inherit", borderColor: surface1, borderWidth: 1 } },
    },
    tooltip: {
      backgroundColor: surface1,
      borderColor: borderSubtle,
      borderWidth: 1,
      textStyle: { color: textPrimary },
      axisPointer: {
        lineStyle: { color: pointer },
        crossStyle: { color: pointer },
        shadowStyle: { color: pointerShadow },
      },
    },
    axisPointer: {
      lineStyle: { color: pointer },
      crossStyle: { color: pointer },
      shadowStyle: { color: pointerShadow },
    },
  };
}

let registered = false;

/** Register both brand themes once. Idempotent. */
export function registerOctoThemes(): void {
  if (registered) return;
  echarts.registerTheme(OCTO_THEME_LIGHT, buildOctoTheme("light"));
  echarts.registerTheme(OCTO_THEME_DARK, buildOctoTheme("dark"));
  registered = true;
}

registerOctoThemes();
