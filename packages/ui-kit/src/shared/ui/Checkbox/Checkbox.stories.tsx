import type { Meta, StoryObj } from "@storybook/react-vite";
import { Checkbox } from "./index";

const meta: Meta<typeof Checkbox> = {
  title: "Form Controls/Checkbox",
  component: Checkbox,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["m", "l"],
    },
    disabled: { control: "boolean" },
    checked: { control: "boolean" },
    indeterminate: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Checkbox>;

export const Default: Story = {
  args: {
    children: "Checkbox label",
  },
};
