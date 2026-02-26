import type { Meta, StoryObj } from "@storybook/react";
import { SegmentedControl } from "@mantine/core";
import { Stack } from "@mantine/core";

const meta: Meta<typeof SegmentedControl> = {
  title: "Form Controls/SegmentedControl",
  component: SegmentedControl,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    orientation: {
      control: "select",
      options: ["horizontal", "vertical"],
    },
    fullWidth: { control: "boolean" },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof SegmentedControl>;

export const Default: Story = {
  args: {
    data: ["JSON", "XML", "Table"],
  },
};

export const FhirVersions: Story = {
  args: {
    data: [
      { label: "R4", value: "r4" },
      { label: "R4B", value: "r4b" },
      { label: "R5", value: "r5" },
      { label: "R6", value: "r6" },
    ],
    value: "r4",
  },
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <SegmentedControl size="xs" data={["One", "Two", "Three"]} />
      <SegmentedControl size="sm" data={["One", "Two", "Three"]} />
      <SegmentedControl size="md" data={["One", "Two", "Three"]} />
      <SegmentedControl size="lg" data={["One", "Two", "Three"]} />
    </Stack>
  ),
};

export const FullWidth: Story = {
  args: {
    data: ["Resources", "Search", "Operations"],
    fullWidth: true,
  },
  decorators: [
    (Story) => (
      <div style={{ width: 400 }}>
        <Story />
      </div>
    ),
  ],
};

export const Disabled: Story = {
  args: {
    data: ["Active", "Inactive", "All"],
    disabled: true,
    value: "Active",
  },
};
