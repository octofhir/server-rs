import type { Meta, StoryObj } from "@storybook/react";
import { Badge } from "@mantine/core";
import { Group, Stack, Text } from "@mantine/core";

const meta: Meta<typeof Badge> = {
  title: "Data Display/Badge",
  component: Badge,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "dot", "default", "transparent"],
    },
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "fire", "warm", "deep", "gray"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Badge>;

export const Default: Story = {
  args: {
    children: "Active",
  },
};

export const AllVariants: Story = {
  render: () => (
    <Group>
      <Badge variant="filled">Filled</Badge>
      <Badge variant="light">Light</Badge>
      <Badge variant="outline">Outline</Badge>
      <Badge variant="dot">Dot</Badge>
      <Badge variant="default">Default</Badge>
      <Badge variant="transparent">Transparent</Badge>
    </Group>
  ),
};

export const Colors: Story = {
  render: () => (
    <Stack>
      <Group>
        <Badge color="primary">Primary</Badge>
        <Badge color="fire">Fire</Badge>
        <Badge color="warm">Warm</Badge>
        <Badge color="deep">Deep</Badge>
        <Badge color="gray">Gray</Badge>
      </Group>
      <Group>
        <Badge variant="filled" color="primary">Primary</Badge>
        <Badge variant="filled" color="fire">Fire</Badge>
        <Badge variant="filled" color="warm">Warm</Badge>
        <Badge variant="filled" color="deep">Deep</Badge>
      </Group>
    </Stack>
  ),
};

export const FhirStatuses: Story = {
  render: () => (
    <Group>
      <Badge color="primary">Active</Badge>
      <Badge color="gray">Inactive</Badge>
      <Badge color="fire">Entered in Error</Badge>
      <Badge color="warm">Draft</Badge>
      <Badge color="primary" variant="filled">Final</Badge>
      <Badge color="deep">Retired</Badge>
    </Group>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Group align="center">
      <Badge size="xs">XS</Badge>
      <Badge size="sm">SM</Badge>
      <Badge size="md">MD</Badge>
      <Badge size="lg">LG</Badge>
      <Badge size="xl">XL</Badge>
    </Group>
  ),
};
