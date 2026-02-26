import type { Meta, StoryObj } from "@storybook/react";
import { Select } from "./Select";
import { Stack } from "@mantine/core";

const meta: Meta<typeof Select> = {
  title: "Form Controls/Select",
  component: Select,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    disabled: { control: "boolean" },
    searchable: { control: "boolean" },
    clearable: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Select>;

const resourceTypes = [
  { value: "Patient", label: "Patient" },
  { value: "Observation", label: "Observation" },
  { value: "Encounter", label: "Encounter" },
  { value: "Condition", label: "Condition" },
  { value: "Procedure", label: "Procedure" },
  { value: "MedicationRequest", label: "MedicationRequest" },
];

export const Default: Story = {
  args: {
    label: "Resource Type",
    placeholder: "Select resource",
    data: resourceTypes,
  },
};

export const Searchable: Story = {
  args: {
    label: "Resource Type",
    placeholder: "Search...",
    data: resourceTypes,
    searchable: true,
  },
};

export const Clearable: Story = {
  args: {
    label: "Resource Type",
    placeholder: "Select resource",
    data: resourceTypes,
    clearable: true,
    value: "Patient",
  },
};

export const WithError: Story = {
  args: {
    label: "Resource Type",
    placeholder: "Select resource",
    data: resourceTypes,
    error: "This field is required",
  },
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <Select size="xs" label="Extra Small" data={resourceTypes} placeholder="xs" />
      <Select size="sm" label="Small" data={resourceTypes} placeholder="sm" />
      <Select size="md" label="Medium" data={resourceTypes} placeholder="md" />
      <Select size="lg" label="Large" data={resourceTypes} placeholder="lg" />
    </Stack>
  ),
};
