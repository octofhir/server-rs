import type { Meta, StoryObj } from "@storybook/react";
import { Checkbox } from "@mantine/core";
import { Stack, Group } from "@mantine/core";

const meta: Meta<typeof Checkbox> = {
  title: "Form Controls/Checkbox",
  component: Checkbox,
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
    indeterminate: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Checkbox>;

export const Default: Story = {
  args: {
    label: "Accept terms and conditions",
  },
};

export const Checked: Story = {
  args: {
    label: "Include inactive resources",
    checked: true,
  },
};

export const Colors: Story = {
  render: () => (
    <Group>
      <Checkbox label="Primary" color="primary" defaultChecked />
      <Checkbox label="Fire" color="fire" defaultChecked />
      <Checkbox label="Warm" color="warm" defaultChecked />
      <Checkbox label="Deep" color="deep" defaultChecked />
    </Group>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <Checkbox size="xs" label="Extra Small" defaultChecked />
      <Checkbox size="sm" label="Small" defaultChecked />
      <Checkbox size="md" label="Medium" defaultChecked />
      <Checkbox size="lg" label="Large" defaultChecked />
    </Stack>
  ),
};

export const States: Story = {
  render: () => (
    <Stack>
      <Checkbox label="Default" />
      <Checkbox label="Checked" defaultChecked />
      <Checkbox label="Indeterminate" indeterminate />
      <Checkbox label="Disabled" disabled />
      <Checkbox label="Disabled checked" disabled defaultChecked />
    </Stack>
  ),
};

export const WithDescription: Story = {
  args: {
    label: "Enable audit logging",
    description: "Records all access to FHIR resources for compliance",
  },
};
