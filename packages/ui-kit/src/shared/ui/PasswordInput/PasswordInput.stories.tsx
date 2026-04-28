import type { Meta, StoryObj } from "@storybook/react-vite";
import { PasswordInput } from "./index";

const meta: Meta<typeof PasswordInput> = {
  title: "Form Controls/PasswordInput",
  component: PasswordInput,
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
type Story = StoryObj<typeof PasswordInput>;

export const Default: Story = {
  args: {
    placeholder: "Enter password",
  },
};
