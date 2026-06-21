import type { Meta, StoryObj } from "@storybook/react-vite";
import { Accordion } from "./index";

const meta: Meta<typeof Accordion> = {
	title: "Data Display/Accordion",
	component: Accordion,
	tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Accordion>;

export const Default: Story = {
	render: () => (
		<Accordion defaultValue={["overview"]} style={{ maxWidth: 480 }}>
			<Accordion.Item value="overview" title="Overview">
				A high-performance FHIR server with a React console.
			</Accordion.Item>
			<Accordion.Item value="storage" title="Storage">
				Resources are stored as JSONB with dynamically created tables.
			</Accordion.Item>
			<Accordion.Item value="auth" title="Authentication">
				OAuth 2.0 and SMART on FHIR with a QuickJS policy engine.
			</Accordion.Item>
		</Accordion>
	),
};

export const Multiple: Story = {
	render: () => (
		<Accordion openMultiple defaultValue={["a", "b"]}>
			<Accordion.Item value="a" title="First">
				Multiple panels can stay open at once.
			</Accordion.Item>
			<Accordion.Item value="b" title="Second">
				Both are expanded here.
			</Accordion.Item>
			<Accordion.Item value="c" title="Third">
				This one starts collapsed.
			</Accordion.Item>
		</Accordion>
	),
};
