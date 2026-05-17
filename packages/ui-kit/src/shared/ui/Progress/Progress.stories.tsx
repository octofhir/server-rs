import type { Meta, StoryObj } from "@storybook/react-vite";
import { Progress } from "./index";

const meta: Meta<typeof Progress> = {
  title: "Feedback/Progress",
  component: Progress,
  tags: ["autodocs"],
  argTypes: {
    theme: {
      control: "select",
      options: ["default", "success", "warning", "danger", "info", "misc"],
    },
    size: {
      control: "select",
      options: ["s", "m"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Progress>;

export const Default: Story = {
  args: {
    value: 50,
    theme: "default",
    size: "m",
  },
};
