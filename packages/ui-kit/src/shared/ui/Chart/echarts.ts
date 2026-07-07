/**
 * Single tree-shaken ECharts entrypoint for the ui-kit. Every chart in the
 * design system imports `echarts` from here so the bundle only pulls the
 * chart/component/renderer modules we actually use.
 *
 * IMPORTANT: this module is listed in the package's `sideEffects` allowlist
 * (see package.json). The `use([...])` call below is a top-level side effect;
 * without the allowlist entry, rolldown-vite's production build treats this
 * module as side-effect-free and strips the registration entirely, leaving
 * zrender's painter registry empty. `echarts.init` then throws
 * `TypeError: <painterCtor> is not a constructor` and no chart renders (this is
 * what broke the notebook dataflow graph — it worked in dev, died in prod).
 * We also call the named `use` import (not `echarts.use`) so the call is never
 * mistaken for a pure namespace-member access.
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
import { use } from "echarts/core";
import { CanvasRenderer, SVGRenderer } from "echarts/renderers";

use([
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
