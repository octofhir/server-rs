import { ActionIcon, Badge, Group, Text, Title } from "@mantine/core";
import { useColorScheme, useLocalStorage } from "@mantine/hooks";
import { IconMoon, IconPalette, IconSun } from "@tabler/icons-react";
import logoUrl from "@/shared/assets/logo.png";

interface HeaderProps {
  height: number;
}

export function Header({ height }: HeaderProps) {
  const systemColorScheme = useColorScheme();
  const [colorScheme, setColorScheme] = useLocalStorage({
    key: "octofhir-color-scheme",
    defaultValue: "auto" as "light" | "dark" | "auto",
  });

  const toggleColorScheme = () => {
    if (colorScheme === "auto") {
      setColorScheme("light");
    } else if (colorScheme === "light") {
      setColorScheme("dark");
    } else {
      setColorScheme("auto");
    }
  };

  const getIcon = () => {
    if (colorScheme === "auto") return <IconPalette size={18} />;
    if (colorScheme === "light") return <IconSun size={18} />;
    return <IconMoon size={18} />;
  };

  return (
    <Group
      h={height}
      px="md"
      justify="space-between"
      style={{ borderBottom: "1px solid var(--mantine-color-gray-3)" }}
    >
      <Group gap="sm">
        <img src={logoUrl} alt="OctoFHIR" style={{ height: 32, width: 32 }} />
        <Title order={3} size="h4">
          OctoFHIR
        </Title>
        <Badge variant="light" size="sm">
          Server UI
        </Badge>
      </Group>

      <Group gap="sm">
        <Text size="sm" c="dimmed">
          {colorScheme === "auto" ? `Auto (${systemColorScheme})` : colorScheme}
        </Text>
        <ActionIcon variant="subtle" onClick={toggleColorScheme} aria-label="Toggle color scheme">
          {getIcon()}
        </ActionIcon>
      </Group>
    </Group>
  );
}
