import { useEffect } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../ui/Button";
import { ToasterHost } from "./ToasterHost";
import { notify } from "./toaster";

const meta: Meta = {
	title: "Feedback/Toaster",
};
export default meta;
type Story = StoryObj;

export const Themes: Story = {
	render: () => {
		useEffect(() => {
			notify.clear();
			notify({ name: "i", theme: "info", title: "Heads up", content: "A background sync just started.", autoHiding: 20000 });
			notify({ name: "s", theme: "success", title: "Saved", content: "Your changes have been persisted.", autoHiding: 20000 });
			notify({ name: "w", theme: "warning", title: "Storage almost full", content: "Free up space to keep ingesting resources.", autoHiding: 20000 });
			notify({ name: "e", theme: "danger", title: "Export failed", content: "Could not reach the FHIR server.", autoHiding: 20000 });
		}, []);
		return (
			<div>
				<Button onClick={() => notify({ theme: "info", title: "Ping", content: "Another toast." })}>
					Add toast
				</Button>
				<ToasterHost />
			</div>
		);
	},
};
