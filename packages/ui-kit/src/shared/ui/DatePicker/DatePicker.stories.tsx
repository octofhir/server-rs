import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { DatePicker, DateTimePicker } from "./index";

const meta: Meta<typeof DatePicker> = {
	title: "Pickers/DatePicker",
	component: DatePicker,
	tags: ["autodocs"],
	argTypes: {
		size: { control: "select", options: ["xs", "s", "m", "l"] },
		withTime: { control: "boolean" },
		clearable: { control: "boolean" },
		disabled: { control: "boolean" },
	},
};

export default meta;
type Story = StoryObj<typeof DatePicker>;

export const Default: Story = {
	render: () => {
		const [date, setDate] = useState<Date | null>(null);
		return <DatePicker label="Date" value={date} onChange={setDate} clearable />;
	},
};

export const WithTime: Story = {
	render: () => {
		const [date, setDate] = useState<Date | null>(new Date(2026, 5, 21, 14, 30));
		return <DateTimePicker label="Timestamp" value={date} onChange={setDate} clearable />;
	},
};
