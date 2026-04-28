import type { Meta, StoryObj } from "@storybook/react";
import { DatePicker } from "./index";

const meta: Meta<typeof DatePicker> = {
  title: "Pickers/DatePicker",
  component: DatePicker,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l", "xl"],
    },
    disabled: { control: "boolean" },
    hasClear: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof DatePicker>;

export const Default: Story = {
  args: {
    placeholder: "Select a date",
  },
};
