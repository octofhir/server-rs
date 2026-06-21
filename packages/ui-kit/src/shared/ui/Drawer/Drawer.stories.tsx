import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../Button";
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
				<Drawer open={open} onClose={() => setOpen(false)} placement="right" title="Details">
					<p>Drawer content goes here.</p>
					<Button onClick={() => setOpen(false)}>Close</Button>
				</Drawer>
			</>
		);
	},
};
