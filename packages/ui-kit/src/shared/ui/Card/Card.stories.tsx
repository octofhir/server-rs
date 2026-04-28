import type { Meta, StoryObj } from "@storybook/react-vite";
import { Card } from "./index";

const meta: Meta<typeof Card> = {
  title: "Data Display/Card",
  component: Card,
  tags: ["autodocs"],
  argTypes: {
    view: {
      control: "select",
      options: ["outlined", "filled", "raised"],
    },
    theme: {
      control: "select",
      options: ["normal", "info", "success", "warning", "danger"],
    },
    type: {
      control: "select",
      options: ["action", "selection"],
    },
    size: {
      control: "select",
      options: ["l", "m"],
    },
    disabled: { control: "boolean" },
    selected: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Card>;

export const Default: Story = {
  args: {
    children: <div style={{ padding: 16 }}>Card content</div>,
    view: "outlined",
  },
};
