import type { Meta, StoryObj } from "@storybook/react-vite";
import { Box } from "./index";

const meta: Meta<typeof Box> = {
  title: "Layout/Box",
  component: Box,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Box>;

export const Default: Story = {
  args: {
    children: <div style={{ padding: 16, backgroundColor: "var(--g-color-base-selection)" }}>Box content</div>,
  },
};
