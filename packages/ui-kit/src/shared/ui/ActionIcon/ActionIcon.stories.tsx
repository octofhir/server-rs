import type { Meta, StoryObj } from "@storybook/react-vite";
import { ActionIcon } from "./ActionIcon";
import { Settings as Gear, Trash2 as TrashBin, Pencil, Copy, RotateCw as ArrowRotateRight, Plus } from "lucide-react";

const meta: Meta<typeof ActionIcon> = {
  title: "Form Controls/ActionIcon",
  component: ActionIcon,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "subtle", "default", "transparent"],
    },
    color: {
      control: "select",
      options: ["primary", "red", "green", "orange", "gray"],
    },
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
    selected: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof ActionIcon>;

export const Default: Story = {
  args: {
    children: <Gear width={18} />,
    variant: "subtle",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <ActionIcon variant="default"><Plus width={18} /></ActionIcon>
      <ActionIcon variant="filled"><Pencil width={18} /></ActionIcon>
      <ActionIcon variant="outline"><Copy width={18} /></ActionIcon>
      <ActionIcon variant="subtle"><Gear width={18} /></ActionIcon>
      <ActionIcon variant="subtle" color="primary"><ArrowRotateRight width={18} /></ActionIcon>
      <ActionIcon variant="subtle" color="red"><TrashBin width={18} /></ActionIcon>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <ActionIcon size="xs"><Gear width={14} /></ActionIcon>
      <ActionIcon size="sm"><Gear width={16} /></ActionIcon>
      <ActionIcon size="md"><Gear width={18} /></ActionIcon>
      <ActionIcon size="lg"><Gear width={20} /></ActionIcon>
      <ActionIcon size="xl"><Gear width={24} /></ActionIcon>
    </div>
  ),
};

export const States: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <ActionIcon variant="subtle"><Gear width={18} /></ActionIcon>
      <ActionIcon variant="subtle" disabled><Gear width={18} /></ActionIcon>
      <ActionIcon variant="subtle" loading><Gear width={18} /></ActionIcon>
      <ActionIcon variant="subtle" selected><Gear width={18} /></ActionIcon>
    </div>
  ),
};
