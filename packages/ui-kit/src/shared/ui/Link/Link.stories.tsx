import type { Meta, StoryObj } from "@storybook/react-vite";
import { Link } from "./index";

const meta: Meta<typeof Link> = {
  title: "Navigation/Link",
  component: Link,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Link>;

export const Default: Story = {
  args: {
    href: "#",
    children: "Navigation Link",
  },
};
