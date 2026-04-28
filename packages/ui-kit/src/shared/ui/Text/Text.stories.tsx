import type { Meta, StoryObj } from "@storybook/react-vite";
import { Text } from "./index";

const meta: Meta<typeof Text> = {
  title: "Data Display/Text",
  component: Text,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: [
        "display-4", "display-3", "display-2", "display-1",
        "header-2", "header-1",
        "subheader-3", "subheader-2", "subheader-1",
        "body-3", "body-2", "body-1", "body-short",
        "caption-2", "caption-1",
      ],
    },
    color: {
      control: "select",
      options: [
        "primary", "secondary", "complementary", "hint",
        "info", "positive", "warning", "danger",
        "link", "brand",
      ],
    },
    ellipsis: { control: "boolean" },
    whiteSpace: {
      control: "select",
      options: ["normal", "nowrap", "pre", "pre-wrap", "pre-line"],
    },
    wordBreak: {
      control: "select",
      options: ["normal", "break-all", "keep-all", "break-word"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Text>;

export const Default: Story = {
  args: {
    children: "Text content",
    variant: "body-1",
  },
};
