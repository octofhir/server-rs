import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@gravity-ui/uikit";
import { Drawer } from "./index";

const meta: Meta<typeof Drawer> = {
    title: "Overlays/Drawer",
    component: Drawer,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Drawer>;

export const Default: Story = {
    render: () => {
        const [open, setOpen] = useState(false);
        return (
            <>
                <Button onClick={() => setOpen(true)}>Open drawer</Button>
                <Drawer open={open} onOpenChange={setOpen} placement="right">
                    <div style={{ padding: 20, width: 320 }}>
                        <h3>Drawer title</h3>
                        <p>Drawer content goes here.</p>
                        <Button onClick={() => setOpen(false)}>Close</Button>
                    </div>
                </Drawer>
            </>
        );
    },
};
