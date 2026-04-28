import type { Meta, StoryObj } from "@storybook/react-vite";
import { TextInput } from "./index";

const meta: Meta<typeof TextInput> = {
  title: "Form Controls/TextInput",
  component: TextInput,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l", "xl"],
    },
    view: {
      control: "select",
      options: ["normal", "clear"],
    },
    pin: {
      control: "select",
      options: ["round-round", "brick-brick", "clear-clear"],
    },
    disabled: { control: "boolean" },
    hasClear: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof TextInput>;

export const Default: Story = {
  args: {
    placeholder: "Enter text...",
  },
};
