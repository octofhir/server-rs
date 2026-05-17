import type { Meta, StoryObj } from "@storybook/react-vite";
import { Skeleton } from "./index";

const meta: Meta<typeof Skeleton> = {
  title: "Feedback/Skeleton",
  component: Skeleton,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Skeleton>;

export const Default: Story = {
  args: {
    style: { width: 200, height: 20 },
  },
};
