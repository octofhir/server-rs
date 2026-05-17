import type { Meta, StoryObj } from "@storybook/react-vite";
import { Hotkey } from "./index";

const meta: Meta<typeof Hotkey> = {
  title: "Data Display/Hotkey",
  component: Hotkey,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Hotkey>;

export const Default: Story = {
  args: {
    value: "mod+s",
  },
};
