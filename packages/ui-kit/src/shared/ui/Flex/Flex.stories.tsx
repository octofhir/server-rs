import type { Meta, StoryObj } from "@storybook/react-vite";
import { Flex } from "./index";

const meta: Meta<typeof Flex> = {
  title: "Layout/Flex",
  component: Flex,
  tags: ["autodocs"],
  argTypes: {
    direction: {
      control: "select",
      options: ["row", "row-reverse", "column", "column-reverse"],
    },
    align: {
      control: "select",
      options: ["flex-start", "flex-end", "center", "baseline", "stretch"],
    },
    justify: {
      control: "select",
      options: ["flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly"],
    },
    wrap: {
      control: "select",
      options: ["nowrap", "wrap", "wrap-reverse"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Flex>;

export const Default: Story = {
  args: {
    direction: "row",
    gap: 4,
    children: (
      <>
        <div style={{ padding: 16, backgroundColor: "var(--g-color-base-selection)" }}>Item 1</div>
        <div style={{ padding: 16, backgroundColor: "var(--g-color-base-selection)" }}>Item 2</div>
        <div style={{ padding: 16, backgroundColor: "var(--g-color-base-selection)" }}>Item 3</div>
      </>
    ),
  },
};
