import type { Meta, StoryObj } from "@storybook/react";
import { ActionIcon } from "@mantine/core";
import { Group, Stack, Text } from "@mantine/core";
import {
  IconSettings,
  IconTrash,
  IconEdit,
  IconCopy,
  IconRefresh,
  IconPlus,
} from "@tabler/icons-react";

const meta: Meta<typeof ActionIcon> = {
  title: "Form Controls/ActionIcon",
  component: ActionIcon,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "subtle", "default", "transparent"],
    },
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "fire", "warm", "deep", "gray"],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof ActionIcon>;

export const Default: Story = {
  args: {
    children: <IconSettings size={18} />,
    variant: "subtle",
  },
};

export const AllVariants: Story = {
  render: () => (
    <Group>
      <ActionIcon variant="filled">
        <IconPlus size={18} />
      </ActionIcon>
      <ActionIcon variant="light">
        <IconEdit size={18} />
      </ActionIcon>
      <ActionIcon variant="outline">
        <IconCopy size={18} />
      </ActionIcon>
      <ActionIcon variant="subtle">
        <IconSettings size={18} />
      </ActionIcon>
      <ActionIcon variant="default">
        <IconRefresh size={18} />
      </ActionIcon>
      <ActionIcon variant="transparent">
        <IconTrash size={18} />
      </ActionIcon>
    </Group>
  ),
};

export const Colors: Story = {
  render: () => (
    <Stack>
      <Group>
        <Text size="sm" w={60}>Filled</Text>
        <ActionIcon variant="filled" color="primary"><IconPlus size={18} /></ActionIcon>
        <ActionIcon variant="filled" color="fire"><IconTrash size={18} /></ActionIcon>
        <ActionIcon variant="filled" color="warm"><IconRefresh size={18} /></ActionIcon>
        <ActionIcon variant="filled" color="deep"><IconSettings size={18} /></ActionIcon>
      </Group>
      <Group>
        <Text size="sm" w={60}>Light</Text>
        <ActionIcon variant="light" color="primary"><IconPlus size={18} /></ActionIcon>
        <ActionIcon variant="light" color="fire"><IconTrash size={18} /></ActionIcon>
        <ActionIcon variant="light" color="warm"><IconRefresh size={18} /></ActionIcon>
        <ActionIcon variant="light" color="deep"><IconSettings size={18} /></ActionIcon>
      </Group>
    </Stack>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Group align="center">
      <ActionIcon size="xs"><IconSettings size={14} /></ActionIcon>
      <ActionIcon size="sm"><IconSettings size={16} /></ActionIcon>
      <ActionIcon size="md"><IconSettings size={18} /></ActionIcon>
      <ActionIcon size="lg"><IconSettings size={20} /></ActionIcon>
      <ActionIcon size="xl"><IconSettings size={24} /></ActionIcon>
    </Group>
  ),
};

export const States: Story = {
  render: () => (
    <Group>
      <ActionIcon variant="light"><IconSettings size={18} /></ActionIcon>
      <ActionIcon variant="light" disabled><IconSettings size={18} /></ActionIcon>
      <ActionIcon variant="light" loading><IconSettings size={18} /></ActionIcon>
    </Group>
  ),
};
