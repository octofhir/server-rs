/**
 * Single tree-shaken ECharts entrypoint for the ui-kit. Every chart in the
 * design system imports `echarts` from here so the bundle only pulls the
 * chart/component/renderer modules we actually use.
 */

import { BarChart, GraphChart, LineChart, PieChart, ScatterChart } from "echarts/charts";
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  TitleComponent,
  ToolboxComponent,
  TooltipComponent,
} from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer, SVGRenderer } from "echarts/renderers";

echarts.use([
  // charts
  BarChart,
  LineChart,
  PieChart,
  ScatterChart,
  GraphChart,
  // components
  GridComponent,
  TooltipComponent,
  LegendComponent,
  TitleComponent,
  DataZoomComponent,
  ToolboxComponent,
  MarkLineComponent,
  // renderers
  CanvasRenderer,
  SVGRenderer,
]);

export { echarts };
export type { EChartsCoreOption, EChartsType } from "echarts/core";
