import { registerOctoThemes } from "./echartsTheme";

type EChartsModule = typeof import("./echarts")["echarts"];

let pending: Promise<EChartsModule> | null = null;

/**
 * Loads the ECharts chunk on first use and registers the brand themes into it.
 * Everything outside this function imports `./echarts` as types only, so pages
 * without a chart never download it.
 */
export function loadEcharts(): Promise<EChartsModule> {
  pending ??= import("./echarts").then(({ echarts }) => {
    registerOctoThemes(echarts);
    return echarts;
  });
  return pending;
}
