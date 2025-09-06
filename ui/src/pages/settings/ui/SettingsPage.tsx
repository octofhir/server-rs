import {
  Button,
  Card,
  Container,
  Group,
  NumberInput,
  Select,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useUnit } from "effector-react";
import { IconCheck, IconPlugConnected, IconPlugConnectedX } from "@tabler/icons-react";
import {
  $apiBaseUrl,
  $apiTimeout,
  $colorScheme,
  setApiBaseUrl,
  setApiTimeout,
  setColorScheme,
} from "@/entities/settings/model";
import { $connectionStatus, getHealthFx } from "@/entities/system";

export function SettingsPage() {
  const [apiBaseUrl, apiTimeout, colorScheme] = useUnit([
    $apiBaseUrl,
    $apiTimeout,
    $colorScheme,
  ]);
  const connectionStatus = useUnit($connectionStatus);
  const healthLoading = useUnit(getHealthFx.pending);

  const statusColor =
    connectionStatus === "connected"
      ? "green"
      : connectionStatus === "connecting"
      ? "yellow"
      : "red";
  const statusIconEl =
    connectionStatus === "connected"
      ? <IconCheck size={16} />
      : connectionStatus === "connecting"
      ? <IconPlugConnected size={16} />
      : <IconPlugConnectedX size={16} />;

  return (
    <Container size="lg">
      <Stack gap="lg">
        <div>
          <Title order={1} size="h2" mb="xs">
            Settings
          </Title>
          <Text c="dimmed">Configure server settings and preferences</Text>
        </div>

        {/* Connection Settings */}
        <Card withBorder radius="lg" p="xl">
          <Stack gap="md">
            <Group justify="space-between" align="center">
              <Title order={3}>Connection</Title>
              <Group gap="sm">
                <Text c={statusColor} fw={600} size="sm" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  {statusIconEl}
                  {connectionStatus.charAt(0).toUpperCase() + connectionStatus.slice(1)}
                </Text>
                <Button size="xs" loading={healthLoading} onClick={() => getHealthFx()}>Test connection</Button>
              </Group>
            </Group>

            <TextInput
              label="FHIR Base URL"
              placeholder="http://localhost:8080"
              value={apiBaseUrl}
              onChange={(e) => setApiBaseUrl(e.currentTarget.value)}
              description="Base URL of your FHIR server (e.g., http://localhost:8080)"
            />

            <NumberInput
              label="Request timeout (ms)"
              value={apiTimeout}
              onChange={(v) => typeof v === "number" && setApiTimeout(v)}
              min={1000}
              step={500}
              clampBehavior="strict"
              description="How long to wait before a request is aborted"
            />
          </Stack>
        </Card>

        {/* Appearance */}
        <Card withBorder radius="lg" p="xl">
          <Stack gap="md">
            <Title order={3}>Appearance</Title>
            <Select
              label="Theme"
              data={[
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
                { value: "auto", label: "System" },
              ]}
              value={colorScheme}
              onChange={(v) => v && setColorScheme(v as "light" | "dark" | "auto")}
              allowDeselect={false}
            />
          </Stack>
        </Card>
      </Stack>
    </Container>
  );
}
