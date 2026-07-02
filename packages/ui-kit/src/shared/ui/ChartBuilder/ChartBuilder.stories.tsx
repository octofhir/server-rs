import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import type { ChartSpec, TabularData } from "../Chart/types";
import { ChartBuilder } from "./ChartBuilder";

const meta: Meta<typeof ChartBuilder> = {
  title: "Data display/ChartBuilder",
  component: ChartBuilder,
  tags: ["autodocs"],
  parameters: { layout: "fullscreen" },
};

export default meta;
type Story = StoryObj<typeof ChartBuilder>;

const timeseries: TabularData = {
  columns: ["month", "reads", "writes", "searches"],
  rows: [
    ["2026-01", 820, 220, 150],
    ["2026-02", 932, 282, 212],
    ["2026-03", 901, 201, 201],
    ["2026-04", 934, 234, 154],
    ["2026-05", 1290, 290, 190],
    ["2026-06", 1330, 430, 330],
  ],
};

const categorical: TabularData = {
  columns: ["resource_type", "region", "count"],
  rows: [
    ["Patient", "us-east", 9120],
    ["Patient", "eu-west", 4300],
    ["Observation", "us-east", 48200],
    ["Observation", "eu-west", 21100],
    ["Encounter", "us-east", 12400],
    ["Encounter", "eu-west", 8700],
    ["Condition", "us-east", 7300],
    ["Condition", "eu-west", 5100],
  ],
};

const sales: TabularData = {
  columns: ["rep", "deals", "revenue", "win_rate"],
  rows: [
    ["Alice", 42, 182000, 0.61],
    ["Bob", 31, 121000, 0.48],
    ["Carol", 55, 240000, 0.72],
    ["Dan", 27, 98000, 0.41],
    ["Erin", 38, 156000, 0.55],
  ],
};

// JSON cells — mirrors `SELECT id, resource FROM patient` from the SQL console.
function patient(
  id: string,
  gender: string,
  birthDate: string,
  family: string,
  systems: string[]
): unknown[] {
  return [
    id,
    {
      resourceType: "Patient",
      gender,
      birthDate,
      name: [{ use: "official", family, given: ["A"] }],
      telecom: systems.map((system) => ({ system, value: `${system}-${id}` })),
    },
  ];
}

const fhirResources: TabularData = {
  columns: ["id", "resource"],
  rows: [
    patient("1", "male", "1985-04-12", "Smith", ["phone", "email"]),
    patient("2", "female", "1990-09-30", "Jones", ["phone"]),
    patient("3", "female", "1978-01-22", "Brown", ["email", "fax", "phone"]),
    patient("4", "male", "2001-07-19", "Davis", ["phone"]),
    patient("5", "other", "1995-12-03", "Wilson", ["email"]),
    patient("6", "female", "1982-06-15", "Moore", ["phone", "email"]),
  ],
};

export const Timeseries: Story = {
  args: { data: timeseries },
};

/**
 * FHIR resources with nested JSON cells. The builder auto-discovers fields like
 * `resource.gender`, `resource.name[0].family`, and the flatten field
 * `resource.telecom[].system` (explodes rows — one per contact method).
 */
export const FhirResources: Story = {
  args: { data: fhirResources },
};

export const Categorical: Story = {
  args: { data: categorical },
};

export const Sales: Story = {
  args: { data: sales },
};

/** Controlled spec — the parent owns and inspects the serializable spec. */
export const Controlled: Story = {
  render: (args) => {
    const [spec, setSpec] = useState<ChartSpec>({
      type: "bar",
      x: "resource_type",
      series: [{ column: "count", agg: "sum" }],
      groupBy: "region",
      stack: true,
      legend: true,
      title: "Resources by type & region",
    });
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 12, padding: 16 }}>
        <ChartBuilder {...args} spec={spec} onSpecChange={setSpec} />
        <pre
          style={{
            margin: 0,
            padding: 12,
            fontSize: 12,
            borderRadius: 8,
            background: "var(--octo-surface-2)",
            color: "var(--octo-text-secondary)",
            overflow: "auto",
          }}
        >
          {JSON.stringify(spec, null, 2)}
        </pre>
      </div>
    );
  },
  args: { data: categorical },
};

/** Edge case — no numeric columns. */
export const NonNumeric: Story = {
  args: {
    data: {
      columns: ["id", "status", "name"],
      rows: [
        ["a1", "active", "Alpha"],
        ["b2", "inactive", "Beta"],
        ["c3", "active", "Gamma"],
      ],
    },
  },
};

/** Edge case — empty dataset. */
export const Empty: Story = {
  args: { data: { columns: [], rows: [] } },
};
