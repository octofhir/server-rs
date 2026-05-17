import {
  ActionIcon,
  Badge,
  Button,
  Card,
  DataPreview,
  Group,
  KeyValueList,
  Modal,
  SectionPanel,
  Stack,
  Text,
  Tooltip,
} from '@/shared/ui';
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { useDisclosure } from '@octofhir/ui-kit';
import {
  Display,
  Smartphone,
  ArrowRightFromSquare,
  ArrowRotateRight,
  TrashBin,
} from '@gravity-ui/icons';
import { useState } from 'react';
import {
  getAuthSessionActivityView,
  getAuthSessionDeviceView,
  isCurrentAuthSession,
  type AuthSession,
  type SessionDeviceKind,
} from '@/entities/auth-session';
import {
  getCurrentSessionToken,
  useRevokeAllSessions,
  useRevokeSession,
  useSessions,
} from '../lib/useSessions';

function DeviceIcon({ kind }: { kind: SessionDeviceKind }) {
  return kind === 'mobile' ? <Smartphone size={20} /> : <Display size={20} />;
}

export function SessionsPage() {
  // FIXME: Get actual user ID from auth context
  const userId = 'current-user-id'; // Replace with actual current user ID from auth context
  const currentSessionToken = getCurrentSessionToken();

  const { data: sessions = [], isLoading, refetch } = useSessions(userId);
  const revokeSessionMutation = useRevokeSession();
  const revokeAllMutation = useRevokeAllSessions();

  const [revokeModalOpened, { open: openRevokeModal, close: closeRevokeModal }] = useDisclosure(false);
  const [revokeAllModalOpened, { open: openRevokeAllModal, close: closeRevokeAllModal }] = useDisclosure(false);
  const [selectedSession, setSelectedSession] = useState<AuthSession | null>(null);

  // Find current session
  const currentSession = sessions.find((s) => isCurrentAuthSession(s, currentSessionToken));

  // Other sessions (not current)
  const otherSessions = sessions.filter((s) => !isCurrentAuthSession(s, currentSessionToken));

  const handleRevokeSession = async (session: AuthSession) => {
    setSelectedSession(session);
    openRevokeModal();
  };

  const confirmRevoke = async () => {
    if (!selectedSession) return;

    try {
      await revokeSessionMutation.mutateAsync(selectedSession.id!);
      closeRevokeModal();
      setSelectedSession(null);
    } catch (error) {
      console.error('Failed to revoke session:', error);
    }
  };

  const handleRevokeAll = () => {
    openRevokeAllModal();
  };

  const confirmRevokeAll = async () => {
    try {
      await revokeAllMutation.mutateAsync({
        userId,
        currentSessionId: currentSession?.id,
      });
      closeRevokeAllModal();
    } catch (error) {
      console.error('Failed to revoke all sessions:', error);
    }
  };

  return (
    <WorkspacePageLayout
      title="Active Sessions"
      description="Manage your active login sessions across devices"
      className="page-enter"
      actions={
          <Group>
            <Button variant="subtle" leftSection={<ArrowRotateRight size={16} />} onClick={() => refetch()}>
              Refresh
            </Button>
            {otherSessions.length > 0 && (
              <Button variant="light" color="red" leftSection={<ArrowRightFromSquare size={16} />} onClick={handleRevokeAll}>
                Revoke All Other Sessions
              </Button>
            )}
          </Group>
      }
    >
      <Stack gap="sm">
        {currentSession && (
          <SectionPanel
            title="Current session"
            description="The browser session currently attached to this control plane"
            view="filled"
            padding="m"
            actions={
              <Badge color="green" size="sm" variant="light">
                Current
              </Badge>
            }
          >
            {(() => {
              const device = getAuthSessionDeviceView(currentSession);
              const activity = getAuthSessionActivityView(currentSession);

              return (
                <Stack gap="sm">
                  <Group>
                    <DeviceIcon kind={device.kind} />
                    <div>
                      <Text fw={600}>{device.deviceName}</Text>
                      <Text size="sm" c="dimmed">
                        {device.browser}
                      </Text>
                    </div>
                  </Group>

                  <KeyValueList
                    items={[
                      { id: 'location', label: 'IP Address', value: activity.location },
                      { id: 'last-active', label: 'Last Active', value: activity.lastActive },
                      { id: 'expires', label: 'Expires', value: activity.expires },
                    ]}
                  />
                </Stack>
              );
            })()}
          </SectionPanel>
        )}

        <SectionPanel
          title="Other sessions"
          description="Active sessions on other browsers and devices"
          view="tinted"
          padding="m"
          actions={
            <Badge size="sm" variant="light" color="gray">
              {otherSessions.length}
            </Badge>
          }
        >
          <DataPreview
            columns={[
              { id: 'device', label: 'Device' },
              { id: 'location', label: 'Location', width: 160 },
              { id: 'lastActive', label: 'Last Active', width: 140 },
              { id: 'expires', label: 'Expires', width: 140 },
              { id: 'actions', label: 'Actions', width: 90 },
            ]}
            rows={
              isLoading
                ? []
                : otherSessions.map((session) => {
                    const device = getAuthSessionDeviceView(session);
                    const activity = getAuthSessionActivityView(session);

                    return {
                      device: (
                        <Group gap="sm">
                          <DeviceIcon kind={device.kind} />
                          <div>
                            <Text size="sm" fw={500}>
                              {device.deviceName}
                            </Text>
                            <Text size="xs" c="dimmed">
                              {device.browser}
                            </Text>
                          </div>
                        </Group>
                      ),
                      location: <Text size="sm">{activity.location}</Text>,
                      lastActive: <Text size="sm">{activity.lastActive}</Text>,
                      expires: <Text size="sm">{activity.expires}</Text>,
                      actions: (
                        <Tooltip label="Revoke session">
                          <ActionIcon
                            variant="subtle"
                            color="red"
                            onClick={() => handleRevokeSession(session)}
                            loading={revokeSessionMutation.isPending}
                          >
                            <TrashBin size={18} />
                          </ActionIcon>
                        </Tooltip>
                      ),
                    };
                  })
            }
            emptyText={isLoading ? 'Loading sessions...' : 'No other active sessions found'}
            getRowKey={(_row, index) => otherSessions[index]?.id ?? `${index}`}
          />
        </SectionPanel>
      </Stack>

      <Modal
        opened={revokeModalOpened}
        onClose={closeRevokeModal}
        title="Revoke Session"
        centered
        radius="lg"
      >
        <Stack gap="sm">
          <Text>
            Are you sure you want to revoke this session? The device will need to sign in again.
          </Text>
          {selectedSession && (
            <Card withBorder radius="md" p="sm">
              {(() => {
                const device = getAuthSessionDeviceView(selectedSession);
                const activity = getAuthSessionActivityView(selectedSession);

                return (
                  <Group gap="sm">
                    <DeviceIcon kind={device.kind} />
                    <div>
                      <Text size="sm" fw={500}>
                        {device.deviceName}
                      </Text>
                      <Text size="xs" c="dimmed">
                        {activity.location}
                      </Text>
                    </div>
                  </Group>
                );
              })()}
            </Card>
          )}
          <Group justify="flex-end" mt="sm">
            <Button variant="subtle" onClick={closeRevokeModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevoke} loading={revokeSessionMutation.isPending}>
              Revoke Session
            </Button>
          </Group>
        </Stack>
      </Modal>

      <Modal
        opened={revokeAllModalOpened}
        onClose={closeRevokeAllModal}
        title="Revoke All Other Sessions"
        centered
        radius="lg"
      >
        <Stack gap="sm">
          <Text>
            Are you sure you want to revoke all other sessions? All other devices will need to sign in again.
          </Text>
          <Text size="sm" c="dimmed">
            This will affect {otherSessions.length} session{otherSessions.length !== 1 ? 's' : ''}.
          </Text>
          <Group justify="flex-end" mt="sm">
            <Button variant="subtle" onClick={closeRevokeAllModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevokeAll} loading={revokeAllMutation.isPending}>
              Revoke All
            </Button>
          </Group>
        </Stack>
      </Modal>
    </WorkspacePageLayout>
  );
}
