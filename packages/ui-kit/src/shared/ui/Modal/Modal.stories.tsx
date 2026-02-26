import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react";
import { Modal } from "@mantine/core";
import { Button, Text, Stack, Group, TextInput } from "@mantine/core";

const meta: Meta<typeof Modal> = {
  title: "Overlays/Modal",
  component: Modal,
  tags: ["autodocs"],
  argTypes: {
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    centered: { control: "boolean" },
    withCloseButton: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof Modal>;

export const Default: Story = {
  render: () => {
    const [opened, setOpened] = useState(false);
    return (
      <>
        <Button onClick={() => setOpened(true)}>Open Modal</Button>
        <Modal opened={opened} onClose={() => setOpened(false)} title="Resource Details">
          <Text size="sm">
            This is a modal with the OctoFHIR theme applied, featuring a blurred
            overlay and custom border radius.
          </Text>
        </Modal>
      </>
    );
  },
};

export const WithForm: Story = {
  render: () => {
    const [opened, setOpened] = useState(false);
    return (
      <>
        <Button onClick={() => setOpened(true)}>Create Resource</Button>
        <Modal opened={opened} onClose={() => setOpened(false)} title="Create Patient">
          <Stack>
            <TextInput label="Family Name" placeholder="Doe" />
            <TextInput label="Given Name" placeholder="John" />
            <TextInput label="Identifier" placeholder="MRN-123456" />
            <Group justify="flex-end">
              <Button variant="default" onClick={() => setOpened(false)}>
                Cancel
              </Button>
              <Button onClick={() => setOpened(false)}>Create</Button>
            </Group>
          </Stack>
        </Modal>
      </>
    );
  },
};

export const Sizes: Story = {
  render: () => {
    const [size, setSize] = useState<string | null>(null);
    return (
      <>
        <Group>
          {(["xs", "sm", "md", "lg", "xl"] as const).map((s) => (
            <Button key={s} variant="light" onClick={() => setSize(s)}>
              {s.toUpperCase()}
            </Button>
          ))}
        </Group>
        <Modal
          opened={size !== null}
          onClose={() => setSize(null)}
          title={`Modal size: ${size}`}
          size={size || "md"}
        >
          <Text size="sm">
            This modal uses size=&quot;{size}&quot;.
          </Text>
        </Modal>
      </>
    );
  },
};

export const Confirmation: Story = {
  render: () => {
    const [opened, setOpened] = useState(false);
    return (
      <>
        <Button color="fire" variant="light" onClick={() => setOpened(true)}>
          Delete Resource
        </Button>
        <Modal
          opened={opened}
          onClose={() => setOpened(false)}
          title="Confirm Deletion"
          size="sm"
        >
          <Text size="sm" mb="lg">
            Are you sure you want to delete this resource? This action cannot be
            undone.
          </Text>
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setOpened(false)}>
              Cancel
            </Button>
            <Button color="fire" onClick={() => setOpened(false)}>
              Delete
            </Button>
          </Group>
        </Modal>
      </>
    );
  },
};
