import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import { Chart } from "./Chart";
import type { EChartsCoreOption } from "./echarts";

const meta: Meta<typeof Chart> = {
  title: "Data display/Chart",
  component: Chart,
  tags: ["autodocs"],
  parameters: { layout: "padded" },
};

export default meta;
type Story = StoryObj<typeof Chart>;

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug"];
const RESOURCE_TYPES = ["Patient", "Observation", "Encounter", "Condition", "Procedure"];

export const Bar: Story = {
  args: {
    "aria-label": "Resources created per month",
    option: {
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: MONTHS },
      yAxis: { type: "value" },
      series: [
        {
          type: "bar",
          name: "Resources",
          data: [1820, 2310, 1990, 2840, 3120, 2760, 3380, 3910],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const HorizontalBar: Story = {
  args: {
    "aria-label": "Resources by type",
    option: {
      tooltip: { trigger: "axis" },
      grid: { left: 90 },
      xAxis: { type: "value" },
      yAxis: { type: "category", data: RESOURCE_TYPES },
      series: [{ type: "bar", name: "Count", data: [9120, 48200, 12400, 7300, 5100] }],
    } satisfies EChartsCoreOption,
  },
};

export const Line: Story = {
  args: {
    "aria-label": "API latency",
    option: {
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", boundaryGap: false, data: MONTHS },
      yAxis: { type: "value" },
      series: [
        {
          type: "line",
          name: "p95 latency (ms)",
          data: [42, 38, 51, 47, 35, 33, 40, 36],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const Area: Story = {
  args: {
    "aria-label": "Active sessions",
    option: {
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", boundaryGap: false, data: MONTHS },
      yAxis: { type: "value" },
      series: [
        {
          type: "line",
          name: "Sessions",
          smooth: true,
          areaStyle: {},
          data: [220, 340, 410, 380, 520, 610, 580, 720],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const MultiSeriesLine: Story = {
  args: {
    "aria-label": "Reads vs writes",
    option: {
      tooltip: { trigger: "axis" },
      legend: {},
      xAxis: { type: "category", boundaryGap: false, data: MONTHS },
      yAxis: { type: "value" },
      series: [
        { type: "line", name: "Reads", data: [820, 932, 901, 934, 1290, 1330, 1320, 1450] },
        { type: "line", name: "Writes", data: [220, 282, 201, 234, 290, 430, 410, 520] },
        { type: "line", name: "Searches", data: [150, 212, 201, 154, 190, 330, 410, 320] },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const StackedBar: Story = {
  args: {
    "aria-label": "Requests by status, stacked",
    option: {
      tooltip: { trigger: "axis" },
      legend: {},
      xAxis: { type: "category", data: MONTHS },
      yAxis: { type: "value" },
      series: [
        {
          type: "bar",
          name: "2xx",
          stack: "total",
          data: [1820, 2310, 1990, 2840, 3120, 2760, 3380, 3910],
        },
        { type: "bar", name: "4xx", stack: "total", data: [120, 90, 140, 80, 110, 70, 95, 60] },
        { type: "bar", name: "5xx", stack: "total", data: [12, 8, 22, 5, 9, 4, 7, 3] },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const Pie: Story = {
  args: {
    "aria-label": "Resource distribution",
    option: {
      tooltip: { trigger: "item" },
      legend: { orient: "vertical", left: "left" },
      series: [
        {
          type: "pie",
          radius: "70%",
          name: "Resources",
          data: RESOURCE_TYPES.map((name, i) => ({
            name,
            value: [9120, 48200, 12400, 7300, 5100][i],
          })),
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const Donut: Story = {
  args: {
    "aria-label": "Storage by tier",
    option: {
      tooltip: { trigger: "item" },
      legend: { bottom: 0 },
      series: [
        {
          type: "pie",
          radius: ["45%", "70%"],
          name: "Storage",
          itemStyle: { borderRadius: 6, borderWidth: 2 },
          data: [
            { name: "Hot", value: 1048 },
            { name: "Warm", value: 735 },
            { name: "Cold", value: 580 },
            { name: "Archive", value: 484 },
          ],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const Scatter: Story = {
  args: {
    "aria-label": "Latency vs payload size",
    option: {
      tooltip: { trigger: "item" },
      xAxis: { type: "value", name: "Payload (KB)" },
      yAxis: { type: "value", name: "Latency (ms)" },
      series: [
        {
          type: "scatter",
          symbolSize: 12,
          data: [
            [10, 22],
            [25, 31],
            [40, 28],
            [55, 49],
            [70, 41],
            [85, 63],
            [100, 58],
            [120, 77],
            [140, 71],
            [160, 92],
          ],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const LiveUpdate: Story = {
  render: (args) => {
    const [data, setData] = useState<number[]>(() => MONTHS.map((_, i) => 1000 + i * 200));
    useEffect(() => {
      const id = setInterval(() => {
        setData((prev) => prev.map((v) => Math.max(0, v + (((v * 7919) % 400) - 200))));
      }, 1200);
      return () => clearInterval(id);
    }, []);
    return (
      <Chart
        {...args}
        option={{
          tooltip: { trigger: "axis" },
          xAxis: { type: "category", data: MONTHS },
          yAxis: { type: "value" },
          series: [{ type: "bar", name: "Live", data }],
        }}
      />
    );
  },
  args: { "aria-label": "Live updating bar chart" },
};

export const DarkMode: Story = {
  globals: { theme: "dark" },
  args: {
    "aria-label": "Multi-series line, dark theme",
    option: {
      tooltip: { trigger: "axis" },
      legend: {},
      xAxis: { type: "category", boundaryGap: false, data: MONTHS },
      yAxis: { type: "value" },
      series: [
        {
          type: "line",
          name: "Reads",
          smooth: true,
          areaStyle: {},
          data: [820, 932, 901, 934, 1290, 1330, 1320, 1450],
        },
        {
          type: "line",
          name: "Writes",
          smooth: true,
          data: [220, 282, 201, 234, 290, 430, 410, 520],
        },
      ],
    } satisfies EChartsCoreOption,
  },
};

export const SvgRenderer: Story = {
  args: {
    renderer: "svg",
    "aria-label": "Bar chart rendered with the SVG backend",
    option: {
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: MONTHS },
      yAxis: { type: "value" },
      series: [
        { type: "bar", name: "SVG", data: [1820, 2310, 1990, 2840, 3120, 2760, 3380, 3910] },
      ],
    } satisfies EChartsCoreOption,
  },
};
