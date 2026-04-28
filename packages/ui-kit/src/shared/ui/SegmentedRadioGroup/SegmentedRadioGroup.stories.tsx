import type { Meta, StoryObj } from "@storybook/react-vite";
import { SegmentedRadioGroup } from "./index";

const meta: Meta<typeof SegmentedRadioGroup> = {
  title: "Form Controls/SegmentedRadioGroup",
  component: SegmentedRadioGroup,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["s", "m", "l", "xl"],
    },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof SegmentedRadioGroup>;

export const Default: Story = {
  args: {
    options: [
      { value: "react", content: "React" },
      { value: "vue", content: "Vue" },
      { value: "angular", content: "Angular" },
    ],
    defaultValue: "react",
  },
};
