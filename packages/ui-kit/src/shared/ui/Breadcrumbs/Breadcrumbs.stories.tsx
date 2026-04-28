import type { Meta, StoryObj } from "@storybook/react-vite";
import { Breadcrumbs } from "./index";

const meta: Meta<typeof Breadcrumbs> = {
    title: "Navigation/Breadcrumbs",
    component: Breadcrumbs,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Breadcrumbs>;

export const Default: Story = {
    render: () => (
        <Breadcrumbs>
            <Breadcrumbs.Item>Home</Breadcrumbs.Item>
            <Breadcrumbs.Item>Products</Breadcrumbs.Item>
            <Breadcrumbs.Item>Details</Breadcrumbs.Item>
        </Breadcrumbs>
    ),
};
