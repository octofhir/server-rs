import type { Meta, StoryObj } from "@storybook/react";
import { ThemeIcon } from "@mantine/core";
import { Group, Stack, Text } from "@mantine/core";
import {
  IconHeart,
  IconUser,
  IconStethoscope,
  IconPill,
  IconActivity,
} from "@tabler/icons-react";

const meta: Meta<typeof ThemeIcon> = {
  title: "Data Display/ThemeIcon",
  component: ThemeIcon,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "default", "transparent"],
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
type Story = StoryObj<typeof ThemeIcon>;

export const Default: Story = {
  args: {
    children: <IconHeart size={18} />,
    variant: "light",
  },
};

export const AllVariants: Story = {
  render: () => (
    <Group>
      <ThemeIcon variant="filled"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon variant="light"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon variant="outline"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon variant="default"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon variant="transparent"><IconHeart size={18} /></ThemeIcon>
    </Group>
  ),
};

export const Colors: Story = {
  render: () => (
    <Stack>
      <Group>
        <Text size="sm" w={60}>Filled</Text>
        <ThemeIcon variant="filled" color="primary"><IconUser size={18} /></ThemeIcon>
        <ThemeIcon variant="filled" color="fire"><IconActivity size={18} /></ThemeIcon>
        <ThemeIcon variant="filled" color="warm"><IconPill size={18} /></ThemeIcon>
        <ThemeIcon variant="filled" color="deep"><IconStethoscope size={18} /></ThemeIcon>
      </Group>
      <Group>
        <Text size="sm" w={60}>Light</Text>
        <ThemeIcon variant="light" color="primary"><IconUser size={18} /></ThemeIcon>
        <ThemeIcon variant="light" color="fire"><IconActivity size={18} /></ThemeIcon>
        <ThemeIcon variant="light" color="warm"><IconPill size={18} /></ThemeIcon>
        <ThemeIcon variant="light" color="deep"><IconStethoscope size={18} /></ThemeIcon>
      </Group>
    </Stack>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Group align="center">
      <ThemeIcon size="xs"><IconHeart size={12} /></ThemeIcon>
      <ThemeIcon size="sm"><IconHeart size={14} /></ThemeIcon>
      <ThemeIcon size="md"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon size="lg"><IconHeart size={22} /></ThemeIcon>
      <ThemeIcon size="xl"><IconHeart size={28} /></ThemeIcon>
    </Group>
  ),
};
