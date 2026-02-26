import type { Meta, StoryObj } from "@storybook/react";
import { Table } from "@mantine/core";

const meta: Meta<typeof Table> = {
  title: "Data Display/Table",
  component: Table,
  tags: ["autodocs"],
  argTypes: {
    striped: { control: "boolean" },
    highlightOnHover: { control: "boolean" },
    withTableBorder: { control: "boolean" },
    withColumnBorders: { control: "boolean" },
    verticalSpacing: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Table>;

const sampleData = [
  { id: "patient-001", name: "John Doe", status: "Active", type: "Patient", updated: "2026-02-25" },
  { id: "patient-002", name: "Jane Smith", status: "Inactive", type: "Patient", updated: "2026-02-24" },
  { id: "obs-001", name: "Blood Pressure", status: "Final", type: "Observation", updated: "2026-02-25" },
  { id: "enc-001", name: "Office Visit", status: "Finished", type: "Encounter", updated: "2026-02-23" },
  { id: "cond-001", name: "Hypertension", status: "Active", type: "Condition", updated: "2026-02-22" },
];

export const Default: Story = {
  render: () => (
    <Table>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>ID</Table.Th>
          <Table.Th>Name</Table.Th>
          <Table.Th>Type</Table.Th>
          <Table.Th>Status</Table.Th>
          <Table.Th>Updated</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {sampleData.map((row) => (
          <Table.Tr key={row.id}>
            <Table.Td>{row.id}</Table.Td>
            <Table.Td>{row.name}</Table.Td>
            <Table.Td>{row.type}</Table.Td>
            <Table.Td>{row.status}</Table.Td>
            <Table.Td>{row.updated}</Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  ),
  decorators: [(Story) => <div style={{ width: 700 }}><Story /></div>],
};

export const Striped: Story = {
  render: () => (
    <Table striped>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Resource</Table.Th>
          <Table.Th>Count</Table.Th>
          <Table.Th>Last Updated</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        <Table.Tr><Table.Td>Patient</Table.Td><Table.Td>1,234</Table.Td><Table.Td>2 min ago</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>Observation</Table.Td><Table.Td>56,789</Table.Td><Table.Td>5 min ago</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>Encounter</Table.Td><Table.Td>12,345</Table.Td><Table.Td>10 min ago</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>Condition</Table.Td><Table.Td>8,901</Table.Td><Table.Td>1 hour ago</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>Procedure</Table.Td><Table.Td>3,456</Table.Td><Table.Td>2 hours ago</Table.Td></Table.Tr>
      </Table.Tbody>
    </Table>
  ),
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};

export const WithBorders: Story = {
  render: () => (
    <Table withTableBorder withColumnBorders>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Parameter</Table.Th>
          <Table.Th>Type</Table.Th>
          <Table.Th>Description</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        <Table.Tr><Table.Td>name</Table.Td><Table.Td>string</Table.Td><Table.Td>Patient name</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>birthdate</Table.Td><Table.Td>date</Table.Td><Table.Td>Date of birth</Table.Td></Table.Tr>
        <Table.Tr><Table.Td>identifier</Table.Td><Table.Td>token</Table.Td><Table.Td>Business identifier</Table.Td></Table.Tr>
      </Table.Tbody>
    </Table>
  ),
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};
