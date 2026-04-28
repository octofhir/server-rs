import type { Meta, StoryObj } from "@storybook/react-vite";
import { Badge } from "./Badge";

const meta: Meta<typeof Badge> = {
  title: "Data Display/Badge",
  component: Badge,
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
type Story = StoryObj<typeof Badge>;

export const Default: Story = {
  args: {
    children: "Badge",
    theme: "normal",
  },
};

export const Themes: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <Badge theme="normal">Normal</Badge>
      <Badge theme="info">Info</Badge>
      <Badge theme="success">Success</Badge>
      <Badge theme="warning">Warning</Badge>
      <Badge theme="danger">Danger</Badge>
      <Badge theme="unknown">Unknown</Badge>
      <Badge theme="clear">Clear</Badge>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <Badge size="xs">Extra Small (xs)</Badge>
      <Badge size="s">Small (s)</Badge>
      <Badge size="m">Medium (m)</Badge>
    </div>
  ),
};

export const Types: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <Badge type="default">Default</Badge>
      <Badge type="copy" copyText="Copied text">Copy</Badge>
      <Badge type="close" onCloseClick={() => alert("Closed!")}>Close</Badge>
    </div>
  ),
};
