import type { Meta, StoryObj } from "@storybook/react-vite";
import { Modal } from "./index";

const meta: Meta<typeof Modal> = {
  title: "Overlays/Modal",
  component: Modal,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Modal>;

export const Default: Story = {
  args: {
    open: true,
    children: <div style={{ padding: 20 }}>Modal content</div>,
  },
};
