import type { Meta, StoryObj } from "@storybook/react";
import { MultiSelect } from "@mantine/core";
import { Stack } from "@mantine/core";

const meta: Meta<typeof MultiSelect> = {
  title: "Form Controls/MultiSelect",
  component: MultiSelect,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    disabled: { control: "boolean" },
    searchable: { control: "boolean" },
    clearable: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof MultiSelect>;

const searchParams = [
  { value: "name", label: "name" },
  { value: "birthdate", label: "birthdate" },
  { value: "gender", label: "gender" },
  { value: "identifier", label: "identifier" },
  { value: "address", label: "address" },
  { value: "phone", label: "phone" },
  { value: "email", label: "email" },
];

export const Default: Story = {
  args: {
    label: "Search Parameters",
    placeholder: "Pick parameters",
    data: searchParams,
  },
};

export const WithValues: Story = {
  args: {
    label: "Search Parameters",
    placeholder: "Pick parameters",
    data: searchParams,
    value: ["name", "birthdate", "gender"],
  },
};

export const Searchable: Story = {
  args: {
    label: "Search Parameters",
    placeholder: "Type to search...",
    data: searchParams,
    searchable: true,
  },
};

export const Sizes: Story = {
  render: () => (
    <Stack>
      <MultiSelect
        size="xs"
        label="Extra Small"
        data={searchParams}
        value={["name"]}
      />
      <MultiSelect
        size="sm"
        label="Small"
        data={searchParams}
        value={["name", "birthdate"]}
      />
      <MultiSelect
        size="md"
        label="Medium"
        data={searchParams}
        value={["name", "birthdate", "gender"]}
      />
    </Stack>
  ),
};
