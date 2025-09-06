import { Group, NavLink, Stack, Text, ThemeIcon } from "@mantine/core";
import { IconApi, IconDatabase, IconSearch, IconSettings } from "@tabler/icons-react";
import { Link, useLocation } from "react-router-dom";

interface SidebarProps {
  width: number;
}

const navigation = [
  {
    label: "Resource Browser",
    path: "/",
    icon: IconDatabase,
    description: "Browse FHIR resources",
  },
  {
    label: "REST Console",
    path: "/console",
    icon: IconApi,
    description: "Test FHIR API endpoints",
  },
  {
    label: "Settings",
    path: "/settings",
    icon: IconSettings,
    description: "Configure server settings",
  },
];

export function Sidebar({ width }: SidebarProps) {
  const location = useLocation();

  return (
    <Stack
      w={width}
      h="100%"
      p="md"
      gap="xs"
      style={{ borderRight: "1px solid var(--mantine-color-gray-3)" }}
    >
      <Group mb="md">
        <ThemeIcon variant="light" size="sm">
          <IconSearch size={16} />
        </ThemeIcon>
        <Text size="sm" fw={600} c="dimmed">
          Navigation
        </Text>
      </Group>

      {navigation.map((item) => (
        <NavLink
          key={item.path}
          component={Link}
          to={item.path}
          label={item.label}
          description={item.description}
          leftSection={<item.icon size={20} />}
          active={location.pathname === item.path}
          style={{ borderRadius: "var(--mantine-radius-md)" }}
        />
      ))}
    </Stack>
  );
}
