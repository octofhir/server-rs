import type { Meta, StoryObj } from "@storybook/react";
import { Button } from "./Button";
import { Group, Stack } from "@mantine/core";
import { IconPlus, IconDownload, IconTrash } from "@tabler/icons-react";

const meta: Meta<typeof Button> = {
  title: "Form Controls/Button",
  component: Button,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: [
        "filled",
        "light",
        "outline",
        "subtle",
        "default",
        "transparent",
        "white",
        "gradient",
      ],
    },
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "fire", "warm", "deep", "gray"],
    },
    radius: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
    fullWidth: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Button>;

export const Default: Story = {
  args: {
    children: "Button",
    variant: "filled",
  },
};

export const AllVariants: Story = {
  render: () => (
    <Group>
      <Button variant="filled">Filled</Button>
      <Button variant="light">Light</Button>
      <Button variant="outline">Outline</Button>
      <Button variant="subtle">Subtle</Button>
      <Button variant="default">Default</Button>
      <Button variant="transparent">Transparent</Button>
      <Button variant="white">White</Button>
      <Button variant="gradient">Gradient</Button>
    </Group>
  ),
};

export const ColorVariants: Story = {
  render: () => (
    <Stack>
      <Group>
        <Button color="primary">Primary</Button>
        <Button color="fire">Fire</Button>
        <Button color="warm">Warm</Button>
        <Button color="deep">Deep</Button>
      </Group>
      <Group>
        <Button variant="light" color="primary">
          Primary
        </Button>
        <Button variant="light" color="fire">
          Fire
        </Button>
        <Button variant="light" color="warm">
          Warm
        </Button>
        <Button variant="light" color="deep">
          Deep
        </Button>
      </Group>
      <Group>
        <Button variant="outline" color="primary">
          Primary
        </Button>
        <Button variant="outline" color="fire">
          Fire
        </Button>
        <Button variant="outline" color="warm">
          Warm
        </Button>
        <Button variant="outline" color="deep">
          Deep
        </Button>
      </Group>
    </Stack>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Group align="center">
      <Button size="xs">Extra Small</Button>
      <Button size="sm">Small</Button>
      <Button size="md">Medium</Button>
      <Button size="lg">Large</Button>
      <Button size="xl">Extra Large</Button>
    </Group>
  ),
};

export const WithIcons: Story = {
  render: () => (
    <Group>
      <Button leftSection={<IconPlus size={16} />}>Create</Button>
      <Button rightSection={<IconDownload size={16} />} variant="light">
        Download
      </Button>
      <Button
        leftSection={<IconTrash size={16} />}
        color="fire"
        variant="light"
      >
        Delete
      </Button>
    </Group>
  ),
};

export const States: Story = {
  render: () => (
    <Group>
      <Button>Normal</Button>
      <Button disabled>Disabled</Button>
      <Button loading>Loading</Button>
    </Group>
  ),
};
