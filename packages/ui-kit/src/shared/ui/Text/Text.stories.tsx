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
        "display-1",
        "header-2", "header-1",
        "subheader-3", "subheader-2", "subheader-1",
        "body-3", "body-2", "body-1",
        "caption-2", "caption-1",
        "code-1", "code-2",
      ],
    },
    color: {
      control: "select",
      options: [
        "primary", "secondary", "muted",
        "info", "success", "warning", "danger",
        "brand", "inherit",
      ],
    },
    ellipsis: { control: "boolean" },
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
