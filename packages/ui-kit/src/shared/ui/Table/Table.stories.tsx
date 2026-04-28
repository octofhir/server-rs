import type { Meta, StoryObj } from "@storybook/react";
import { Table } from "./index";

const meta: Meta<typeof Table> = {
  title: "Data Display/Table",
  component: Table,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Table>;

export const Default: Story = {
  args: {
    columns: [{ id: "name", name: "Name" }, { id: "age", name: "Age" }],
    data: [{ name: "Alice", age: 25 }, { name: "Bob", age: 30 }],
  },
};
