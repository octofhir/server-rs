import type { Meta, StoryObj } from "@storybook/react-vite";
import { ClipboardButton } from "./index";

const meta: Meta<typeof ClipboardButton> = {
  title: "Buttons/ClipboardButton",
  component: ClipboardButton,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof ClipboardButton>;

export const Default: Story = {
  args: {
    text: "Text to copy",
  },
};
