import type { Meta, StoryObj } from "@storybook/react";
import { Card } from "./Card";
import { Text, Group, Badge, Button, Stack } from "@mantine/core";

const meta: Meta<typeof Card> = {
  title: "Data Display/Card",
  component: Card,
  tags: ["autodocs"],
  argTypes: {
    padding: {
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
type Story = StoryObj<typeof Card>;

export const Default: Story = {
  args: {
    children: "A simple card with default OctoFHIR styling.",
    padding: "md",
    style: { maxWidth: 340 },
  },
};

export const Clickable: Story = {
  render: () => (
    <Card onClick={() => {}} style={{ maxWidth: 340 }}>
      <Text fw={600} mb="xs">
        Clickable Card
      </Text>
      <Text size="sm" c="dimmed">
        Hover to see the elevation effect. Click to trigger the action.
      </Text>
    </Card>
  ),
};

export const WithContent: Story = {
  render: () => (
    <Card style={{ maxWidth: 380 }}>
      <Group justify="space-between" mb="sm">
        <Text fw={600}>Patient Resource</Text>
        <Badge color="primary">Active</Badge>
      </Group>
      <Text size="sm" c="dimmed" mb="md">
        John Doe — MRN: 123456
      </Text>
      <Button variant="light" fullWidth>
        View Details
      </Button>
    </Card>
  ),
};

export const CardGrid: Story = {
  render: () => (
    <Group>
      {["Patient", "Observation", "Encounter"].map((type) => (
        <Card key={type} onClick={() => {}} style={{ width: 200 }}>
          <Text fw={600} mb={4}>
            {type}
          </Text>
          <Text size="xs" c="dimmed">
            {Math.floor(Math.random() * 10000)} resources
          </Text>
        </Card>
      ))}
    </Group>
  ),
};

export const Shadows: Story = {
  render: () => (
    <Group>
      <Card shadow="xs" style={{ width: 140 }}>
        <Text size="sm" ta="center">shadow=xs</Text>
      </Card>
      <Card shadow="sm" style={{ width: 140 }}>
        <Text size="sm" ta="center">shadow=sm</Text>
      </Card>
      <Card shadow="md" style={{ width: 140 }}>
        <Text size="sm" ta="center">shadow=md</Text>
      </Card>
      <Card shadow="lg" style={{ width: 140 }}>
        <Text size="sm" ta="center">shadow=lg</Text>
      </Card>
    </Group>
  ),
};
