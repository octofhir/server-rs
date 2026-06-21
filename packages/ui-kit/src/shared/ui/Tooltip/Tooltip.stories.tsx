import type { Meta, StoryObj } from "@storybook/react-vite";
import { Tooltip } from "./index";

const meta: Meta<typeof Tooltip> = {
  title: "Overlays/Tooltip",
  component: Tooltip,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Tooltip>;

export const Default: Story = {
  args: {
    label: "Tooltip content",
    children: <span>Hover me</span>,
  },
};
