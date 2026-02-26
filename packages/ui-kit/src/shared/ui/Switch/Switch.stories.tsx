import type { Meta, StoryObj } from "@storybook/react";
import { Switch } from "@mantine/core";
import { Stack, Group } from "@mantine/core";

const meta: Meta<typeof Switch> = {
  title: "Form Controls/Switch",
  component: Switch,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "fire", "warm", "deep"],
    },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Switch>;

export const Default: Story = {
  args: {
    label: "Enable notifications",
  },
};

export const Checked: Story = {
  args: {
    label: "Active",
    checked: true,
  },
};

export const Colors: Story = {
  render: () => (
    <Stack>
      <Switch label="Primary" color="primary" defaultChecked />
      <Switch label="Fire" color="fire" defaultChecked />
      <Switch label="Warm" color="warm" defaultChecked />
      <Switch label="Deep" color="deep" defaultChecked />
    </Stack>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <Switch size="xs" label="Extra Small" />
      <Switch size="sm" label="Small" />
      <Switch size="md" label="Medium" />
      <Switch size="lg" label="Large" />
    </Stack>
  ),
};

export const States: Story = {
  render: () => (
    <Stack>
      <Switch label="Default" />
      <Switch label="Checked" defaultChecked />
      <Switch label="Disabled" disabled />
      <Switch label="Disabled checked" disabled defaultChecked />
    </Stack>
  ),
};

export const WithDescription: Story = {
  args: {
    label: "SMART on FHIR",
    description: "Enable SMART App Launch authorization",
  },
};
