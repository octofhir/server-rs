import type { Meta, StoryObj } from "@storybook/react";
import { TextInput } from "./TextInput";
import { Stack } from "@mantine/core";
import { IconSearch, IconAt } from "@tabler/icons-react";

const meta: Meta<typeof TextInput> = {
  title: "Form Controls/TextInput",
  component: TextInput,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    radius: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof TextInput>;

export const Default: Story = {
  args: {
    label: "Name",
    placeholder: "Enter your name",
  },
};

export const WithDescription: Story = {
  args: {
    label: "Email",
    description: "We will never share your email",
    placeholder: "your@email.com",
  },
};

export const WithError: Story = {
  args: {
    label: "Email",
    placeholder: "your@email.com",
    error: "Invalid email address",
    value: "not-an-email",
  },
};

export const WithIcons: Story = {
  render: () => (
    <Stack>
      <TextInput
        label="Search"
        placeholder="Search resources..."
        leftSection={<IconSearch size={16} />}
      />
      <TextInput
        label="Email"
        placeholder="your@email.com"
        leftSection={<IconAt size={16} />}
      />
    </Stack>
  ),
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <TextInput size="xs" label="Extra Small" placeholder="xs" />
      <TextInput size="sm" label="Small" placeholder="sm" />
      <TextInput size="md" label="Medium" placeholder="md" />
      <TextInput size="lg" label="Large" placeholder="lg" />
    </Stack>
  ),
};

export const Disabled: Story = {
  args: {
    label: "Disabled",
    placeholder: "Cannot edit",
    disabled: true,
    value: "Read-only value",
  },
};
