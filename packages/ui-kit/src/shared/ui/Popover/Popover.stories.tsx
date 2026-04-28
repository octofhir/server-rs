import type { Meta, StoryObj } from "@storybook/react";
import { Popover } from "./index";

const meta: Meta<typeof Popover> = {
  title: "Overlays/Popover",
  component: Popover,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Popover>;

export const Default: Story = {
  args: {
    content: <div style={{ padding: 10 }}>Popover content</div>,
    children: <span>Click me</span>,
  },
};
