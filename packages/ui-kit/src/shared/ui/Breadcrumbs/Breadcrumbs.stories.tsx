import type { Meta, StoryObj } from "@storybook/react-vite";
import { Link as Anchor } from "../Link";
import { Text } from "../Text";
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
			<Anchor href="#">Home</Anchor>
			<Anchor href="#">Products</Anchor>
			<Text>Details</Text>
		</Breadcrumbs>
	),
};
