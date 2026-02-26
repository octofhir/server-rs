import type { Meta, StoryObj } from "@storybook/react";
import { Paper } from "@mantine/core";
import { Text, Group, Stack } from "@mantine/core";

const meta: Meta<typeof Paper> = {
  title: "Data Display/Paper",
  component: Paper,
  tags: ["autodocs"],
  argTypes: {
    p: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    radius: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    shadow: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    withBorder: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Paper>;

export const Default: Story = {
  args: {
    p: "md",
    children: "Paper is a surface container with theme-aware styling.",
    style: { maxWidth: 400 },
  },
};

export const WithBorder: Story = {
  args: {
    p: "md",
    withBorder: true,
    children: "Paper with a subtle border.",
    style: { maxWidth: 400 },
  },
};

export const Shadows: Story = {
  render: () => (
    <Group>
      {(["xs", "sm", "md", "lg", "xl"] as const).map((s) => (
        <Paper key={s} shadow={s} p="md" style={{ width: 120 }}>
          <Text size="sm" ta="center">{s}</Text>
        </Paper>
      ))}
    </Group>
  ),
};

export const Nested: Story = {
  render: () => (
    <Paper p="lg" shadow="sm" style={{ maxWidth: 400 }}>
      <Text fw={600} mb="sm">Outer Paper</Text>
      <Stack gap="sm">
        <Paper p="sm" withBorder>
          <Text size="sm">Inner section 1</Text>
        </Paper>
        <Paper p="sm" withBorder>
          <Text size="sm">Inner section 2</Text>
        </Paper>
      </Stack>
    </Paper>
  ),
};
