import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./Button";

const meta: Meta<typeof Button> = {
  title: "Form Controls/Button",
  component: Button,
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
    pin: {
      control: "select",
      options: [
        "round-round",
        "brick-brick",
        "clear-clear",
        "round-brick",
        "brick-round",
        "round-clear",
        "clear-round",
        "brick-clear",
        "clear-brick",
      ],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
    selected: { control: "boolean" },
    width: {
      control: "select",
      options: ["auto", "max"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Button>;

export const Default: Story = {
  args: {
    children: "Button",
    view: "normal",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <Button view="normal">Normal</Button>
      <Button view="action">Action</Button>
      <Button view="outlined">Outlined</Button>
      <Button view="outlined-info">Outlined Info</Button>
      <Button view="outlined-danger">Outlined Danger</Button>
      <Button view="raised">Raised</Button>
      <Button view="flat">Flat</Button>
      <Button view="flat-info">Flat Info</Button>
      <Button view="flat-danger">Flat Danger</Button>
      <Button view="flat-secondary">Flat Secondary</Button>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <Button size="xs">Extra Small (xs)</Button>
      <Button size="s">Small (s)</Button>
      <Button size="m">Medium (m)</Button>
      <Button size="l">Large (l)</Button>
      <Button size="xl">Extra Large (xl)</Button>
    </div>
  ),
};

export const States: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <Button>Normal</Button>
      <Button disabled>Disabled</Button>
      <Button loading>Loading</Button>
      <Button selected>Selected</Button>
    </div>
  ),
};

export const Pins: Story = {
  render: () => (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      <div style={{ display: "flex", gap: "1px" }}>
        <Button pin="round-brick">Round Brick</Button>
        <Button pin="brick-brick">Brick Brick</Button>
        <Button pin="brick-round">Brick Round</Button>
      </div>
      <div style={{ display: "flex", gap: "1px" }}>
        <Button pin="round-clear">Round Clear</Button>
        <Button pin="clear-clear">Clear Clear</Button>
        <Button pin="clear-round">Clear Round</Button>
      </div>
    </div>
  ),
};
