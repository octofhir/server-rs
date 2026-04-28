import type { Meta, StoryObj } from "@storybook/react";
import { Select } from "./index";

const meta: Meta<typeof Select> = {
  title: "Form Controls/Select",
  component: Select,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l", "xl"],
    },
    disabled: { control: "boolean" },
    multiple: { control: "boolean" },
    filterable: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Select>;

export const Default: Story = {
  args: {
    placeholder: "Select an option",
    options: [
      { value: "react", content: "React" },
      { value: "vue", content: "Vue" },
      { value: "angular", content: "Angular" },
    ],
  },
};
