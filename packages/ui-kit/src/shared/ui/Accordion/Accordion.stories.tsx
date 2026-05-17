import type { Meta, StoryObj } from "@storybook/react-vite";
import { Accordion } from "./index";

const meta: Meta<typeof Accordion> = {
  title: "Data Display/Accordion",
  component: Accordion,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Accordion>;

export const Default: Story = {
  args: {},
};
