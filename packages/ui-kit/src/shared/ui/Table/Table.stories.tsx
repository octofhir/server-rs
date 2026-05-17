import type { Meta, StoryObj } from "@storybook/react-vite";
import { Table } from "./index";

const meta: Meta<typeof Table> = {
  title: "Data Display/Table",
  component: Table,
  tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Table>;

export const Default: Story = {
  render: () => (
    <Table striped highlightOnHover withTableBorder>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Name</Table.Th>
          <Table.Th>Age</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        <Table.Tr>
          <Table.Td>Alice</Table.Td>
          <Table.Td>25</Table.Td>
        </Table.Tr>
        <Table.Tr>
          <Table.Td>Bob</Table.Td>
          <Table.Td>30</Table.Td>
        </Table.Tr>
      </Table.Tbody>
    </Table>
  ),
};
