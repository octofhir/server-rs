import type { Meta, StoryObj } from "@storybook/react-vite";
import { Chip } from "./Chip";

const meta: Meta<typeof Chip> = {
  title: "Data Display/Chip",
  component: Chip,
  tags: ["autodocs"],
  argTypes: {
    theme: {
      control: "select",
      options: [
        "normal",
        "info",
        "danger",
        "warning",
        "success",
        "unknown",
        "clear",
      ],
    },
    size: {
      control: "select",
      options: ["xs", "s", "m"],
    },
    type: {
      control: "select",
      options: ["default", "copy", "close"],
    },
    interactive: { control: "boolean" },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Chip>;

export const Default: Story = {
  args: {
    children: "Chip",
    theme: "normal",
    interactive: true,
  },
};

export const Types: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <Chip type="default" interactive>Default</Chip>
      <Chip type="copy" copyText="Copied text" interactive>Copy</Chip>
      <Chip type="close" onCloseClick={() => alert("Closed!")} interactive>Close</Chip>
    </div>
  ),
};
