import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Tabs } from "./Tabs";

const meta: Meta<typeof Tabs> = {
    title: "Navigation/Tabs",
    component: Tabs,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Tabs>;

export const Default: Story = {
    render: () => {
        const [value, setValue] = useState("overview");
        return (
            <Tabs value={value} onChange={(next) => setValue(next ?? "")}>
                <Tabs.List>
                    <Tabs.Tab value="overview">Overview</Tabs.Tab>
                    <Tabs.Tab value="details">Details</Tabs.Tab>
                    <Tabs.Tab value="activity">Activity</Tabs.Tab>
                </Tabs.List>
                <div style={{ paddingTop: 16 }}>
                    <Tabs.Panel value="overview">Overview content</Tabs.Panel>
                    <Tabs.Panel value="details">Details content</Tabs.Panel>
                    <Tabs.Panel value="activity">Activity content</Tabs.Panel>
                </div>
            </Tabs>
        );
    },
};
