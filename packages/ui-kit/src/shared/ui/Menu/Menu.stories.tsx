import type { Meta, StoryObj } from "@storybook/react";
import { Menu } from "./index";
import { Button } from "../Button";

const meta: Meta<typeof Menu> = {
  title: "Overlays/Menu",
  component: Menu,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Menu>;

export const Default: Story = {
  args: {
    items: [
      { action: () => alert('1'), text: 'Item 1' },
      { action: () => alert('2'), text: 'Item 2' },
    ],
    children: <Button>Open Menu</Button>,
  },
};
