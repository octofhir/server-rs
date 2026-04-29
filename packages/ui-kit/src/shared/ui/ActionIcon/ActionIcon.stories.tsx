import type { Meta, StoryObj } from "@storybook/react-vite";
import { ActionIcon } from "./ActionIcon";
import {
  Gear,
  TrashBin,
  Pencil,
  Copy,
  ArrowRotateRight,
  Plus,
} from "@gravity-ui/icons";

const meta: Meta<typeof ActionIcon> = {
  title: "Form Controls/ActionIcon",
  component: ActionIcon,
  tags: ["autodocs"],
  argTypes: {
    view: {
      control: "select",
      options: [
        "normal",
        "action",
        "outlined",
        "outlined-info",
        "outlined-danger",
        "raised",
        "flat",
        "flat-info",
        "flat-danger",
        "flat-secondary",
        "normal-contrast",
        "outlined-contrast",
        "flat-contrast",
      ],
    },
    size: {
      control: "select",
      options: ["xs", "s", "m", "l", "xl"],
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
    view: "flat",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <ActionIcon view="normal"><Plus width={18} /></ActionIcon>
      <ActionIcon view="action"><Pencil width={18} /></ActionIcon>
      <ActionIcon view="outlined"><Copy width={18} /></ActionIcon>
      <ActionIcon view="flat"><Gear width={18} /></ActionIcon>
      <ActionIcon view="flat-info"><ArrowRotateRight width={18} /></ActionIcon>
      <ActionIcon view="flat-danger"><TrashBin width={18} /></ActionIcon>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <ActionIcon size="xs"><Gear width={14} /></ActionIcon>
      <ActionIcon size="s"><Gear width={16} /></ActionIcon>
      <ActionIcon size="m"><Gear width={18} /></ActionIcon>
      <ActionIcon size="l"><Gear width={20} /></ActionIcon>
      <ActionIcon size="xl"><Gear width={24} /></ActionIcon>
    </div>
  ),
};

export const States: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <ActionIcon view="flat"><Gear width={18} /></ActionIcon>
      <ActionIcon view="flat" disabled><Gear width={18} /></ActionIcon>
      <ActionIcon view="flat" loading><Gear width={18} /></ActionIcon>
      <ActionIcon view="flat" selected><Gear width={18} /></ActionIcon>
    </div>
  ),
};
