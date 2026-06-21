import type { Meta, StoryObj } from "@storybook/react-vite";
import { Select } from "./index";

const meta: Meta<typeof Select> = {
    title: "Form Controls/Select",
    component: Select,
    tags: ["autodocs"],
    argTypes: {
        size: { control: "select", options: ["s", "m", "l"] },
        disabled: { control: "boolean" },
    },
};

export default meta;
type Story = StoryObj<typeof Select>;

export const Default: Story = {
    args: {
        placeholder: "Select an option",
        data: [
            { value: "react", label: "React" },
            { value: "vue", label: "Vue" },
            { value: "angular", label: "Angular" },
        ],
    },
};
