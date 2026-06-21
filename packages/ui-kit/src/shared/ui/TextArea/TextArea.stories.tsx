import type { Meta, StoryObj } from "@storybook/react-vite";
import { TextArea } from "./index";

const meta: Meta<typeof TextArea> = {
  title: "Form Controls/TextArea",
  component: TextArea,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l"],
    },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof TextArea>;

export const Default: Story = {
  args: {
    placeholder: "Enter long text...",
    minRows: 3,
  },
};
