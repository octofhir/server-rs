import type { Meta, StoryObj } from "@storybook/react-vite";
import { Portal } from "./index";

const meta: Meta<typeof Portal> = {
  title: "Overlays/Portal",
  component: Portal,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Portal>;

export const Default: Story = {
  args: {
    children: "Portal content",
  },
};
