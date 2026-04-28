import type { Meta, StoryObj } from "@storybook/react-vite";
import { NumberInput } from "./index";

const meta: Meta<typeof NumberInput> = {
  title: "Form Controls/NumberInput",
  component: NumberInput,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l", "xl"],
    },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof NumberInput>;

export const Default: Story = {
  args: {
    placeholder: "0",
  },
};
