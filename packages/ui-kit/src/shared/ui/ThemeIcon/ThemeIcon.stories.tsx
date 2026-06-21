import type { Meta, StoryObj } from "@storybook/react-vite";
import { Heart, User as Person, Pill, Activity as Pulse, Stethoscope } from "lucide-react";
import { ThemeIcon } from "./ThemeIcon";

const meta: Meta<typeof ThemeIcon> = {
  title: "Data Display/ThemeIcon",
  component: ThemeIcon,
  tags: ["autodocs"],
  argTypes: {
    view: {
      control: "select",
      options: ["normal", "light", "outlined"],
    },
    size: {
      control: "select",
      options: ["xs", "s", "m", "l", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "positive", "warning", "danger", "neutral"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof ThemeIcon>;

export const Default: Story = {
  args: {
    children: <Heart width={18} />,
    view: "light",
    color: "primary",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <ThemeIcon view="normal"><Heart width={18} /></ThemeIcon>
      <ThemeIcon view="light"><Heart width={18} /></ThemeIcon>
      <ThemeIcon view="outlined"><Heart width={18} /></ThemeIcon>
    </div>
  ),
};

export const Colors: Story = {
  render: () => (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Normal</span>
        <ThemeIcon view="normal" color="primary"><Person width={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="positive"><Pulse width={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="warning"><Pill width={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="danger"><Stethoscope width={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="neutral"><Heart width={18} /></ThemeIcon>
      </div>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Light</span>
        <ThemeIcon view="light" color="primary"><Person width={18} /></ThemeIcon>
        <ThemeIcon view="light" color="positive"><Pulse width={18} /></ThemeIcon>
        <ThemeIcon view="light" color="warning"><Pill width={18} /></ThemeIcon>
        <ThemeIcon view="light" color="danger"><Stethoscope width={18} /></ThemeIcon>
        <ThemeIcon view="light" color="neutral"><Heart width={18} /></ThemeIcon>
      </div>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Outlined</span>
        <ThemeIcon view="outlined" color="primary"><Person width={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="positive"><Pulse width={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="warning"><Pill width={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="danger"><Stethoscope width={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="neutral"><Heart width={18} /></ThemeIcon>
      </div>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <ThemeIcon size="xs"><Heart width={12} /></ThemeIcon>
      <ThemeIcon size="sm"><Heart width={14} /></ThemeIcon>
      <ThemeIcon size="md"><Heart width={18} /></ThemeIcon>
      <ThemeIcon size="lg"><Heart width={22} /></ThemeIcon>
      <ThemeIcon size="xl"><Heart width={28} /></ThemeIcon>
    </div>
  ),
};
