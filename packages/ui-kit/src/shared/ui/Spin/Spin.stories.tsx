import type { Meta, StoryObj } from "@storybook/react-vite";
import { Spin } from "./index";

const meta: Meta<typeof Spin> = {
  title: "Feedback/Spin",
  component: Spin,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Spin>;

export const Default: Story = {
  args: {
    size: "md",
  },
};
