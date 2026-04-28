import type { Meta, StoryObj } from "@storybook/react";
import { Alert } from "./index";

const meta: Meta<typeof Alert> = {
  title: "Feedback/Alert",
  component: Alert,
  tags: ["autodocs"],
  argTypes: {
    theme: {
      control: "select",
      options: ["normal", "info", "success", "warning", "danger", "utility"],
    },
    view: {
      control: "select",
      options: ["filled", "outlined"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Alert>;

export const Default: Story = {
  args: {
    title: "Alert Title",
    message: "This is an alert message.",
    theme: "info",
    view: "filled",
  },
};
