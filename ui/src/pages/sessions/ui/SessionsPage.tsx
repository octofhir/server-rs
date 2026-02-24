import {
  ActionIcon,
  Badge,
  Button,
  Card,
  Container,
  Flex,
  Group,
  LoadingOverlay,
  Modal,
  Stack,
  Table,
  Text,
  Title,
  Tooltip,
} from '@/shared/ui';
import { useDisclosure } from '@octofhir/ui-kit';
import {
  IconDeviceDesktop,
  IconDeviceMobile,
  IconDeviceTablet,
  IconLogout,
  IconRefresh,
  IconTrash,
} from '@tabler/icons-react';
import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import { useState } from 'react';

dayjs.extend(relativeTime);
import type { AuthSession } from '../lib/useSessions';
import {
  extractUserId,
  getCurrentSessionToken,
  isCurrentSession,
  useRevokeAllSessions,
  useRevokeSession,
  useSessions,
} from '../lib/useSessions';

/**
 * Get device icon based on User-Agent string
 */
function getDeviceIcon(userAgent: string = '') {
  const ua = userAgent.toLowerCase();
  if (ua.includes('mobile') || ua.includes('android') || ua.includes('iphone')) {
    return <IconDeviceMobile size={20} />;
  }
  if (ua.includes('tablet') || ua.includes('ipad')) {
    return <IconDeviceTablet size={20} />;
  }
  return <IconDeviceDesktop size={20} />;
}

/**
 * Format browser name from User-Agent
 */
function formatBrowserName(userAgent: string = ''): string {
  const ua = userAgent.toLowerCase();
  if (ua.includes('chrome')) return 'Chrome';
  if (ua.includes('safari')) return 'Safari';
  if (ua.includes('firefox')) return 'Firefox';
  if (ua.includes('edge')) return 'Edge';
  return 'Unknown Browser';
}

/**
 * Session management page component
 */
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
  const currentSession = sessions.find((s) => isCurrentSession(s, currentSessionToken));

  // Other sessions (not current)
  const otherSessions = sessions.filter((s) => !isCurrentSession(s, currentSessionToken));

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
    <Container size="xl" className="page-enter">
      <Stack gap="xl">
        {/* Header */}
        <Flex justify="space-between" align="center">
          <div>
            <Title order={2}>Active Sessions</Title>
            <Text c="dimmed" size="sm" mt={4}>
              Manage your active login sessions across devices
            </Text>
          </div>
          <Group>
            <Button variant="subtle" leftSection={<IconRefresh size={16} />} onClick={() => refetch()}>
              Refresh
            </Button>
            {otherSessions.length > 0 && (
              <Button variant="light" color="red" leftSection={<IconLogout size={16} />} onClick={handleRevokeAll}>
                Revoke All Other Sessions
              </Button>
            )}
          </Group>
        </Flex>

        {/* Current Session Card (Glass Effect) */}
        {currentSession && (
          <Card
            shadow="sm"
            radius="lg"
            withBorder
            style={{
              background: 'var(--octo-surface-1)',
              border: '1px solid var(--octo-border-subtle)',
            }}
          >
            <Stack gap="md">
              <Group justify="space-between">
                <Group>
                  {getDeviceIcon(currentSession.userAgent)}
                  <div>
                    <Group gap={8}>
                      <Text fw={600}>{currentSession.deviceName || 'Current Device'}</Text>
                      <Badge color="green" size="sm" variant="light">
                        Current Session
                      </Badge>
                    </Group>
                    <Text size="sm" c="dimmed">
                      {formatBrowserName(currentSession.userAgent)}
                    </Text>
                  </div>
                </Group>
              </Group>

              <Group gap="xl">
                {currentSession.ipAddress && (
                  <div>
                    <Text size="xs" c="dimmed">
                      IP Address
                    </Text>
                    <Text size="sm">{currentSession.ipAddress}</Text>
                  </div>
                )}
                <div>
                  <Text size="xs" c="dimmed">
                    Last Active
                  </Text>
                  <Text size="sm">
                    {currentSession.lastActivityAt
                      ? dayjs(currentSession.lastActivityAt).fromNow()
                      : 'Just now'}
                  </Text>
                </div>
                <div>
                  <Text size="xs" c="dimmed">
                    Expires
                  </Text>
                  <Text size="sm">
                    {dayjs(currentSession.expiresAt).fromNow()}
                  </Text>
                </div>
              </Group>
            </Stack>
          </Card>
        )}

        {/* Other Sessions Table */}
        {otherSessions.length > 0 ? (
          <Card shadow="sm" radius="lg" withBorder>
            <LoadingOverlay visible={isLoading} />
            <Stack gap="md">
              <Title order={4}>Other Sessions</Title>
              <Table.ScrollContainer minWidth={700}>
                <Table highlightOnHover>
                  <Table.Thead>
                    <Table.Tr>
                      <Table.Th>Device</Table.Th>
                      <Table.Th>Location</Table.Th>
                      <Table.Th>Last Active</Table.Th>
                      <Table.Th>Expires</Table.Th>
                      <Table.Th>Actions</Table.Th>
                    </Table.Tr>
                  </Table.Thead>
                  <Table.Tbody>
                    {otherSessions.map((session) => (
                      <Table.Tr key={session.id}>
                        <Table.Td>
                          <Group gap="sm">
                            {getDeviceIcon(session.userAgent)}
                            <div>
                              <Text size="sm" fw={500}>
                                {session.deviceName || 'Unknown Device'}
                              </Text>
                              <Text size="xs" c="dimmed">
                                {formatBrowserName(session.userAgent)}
                              </Text>
                            </div>
                          </Group>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm">{session.ipAddress || 'Unknown'}</Text>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm">
                            {session.lastActivityAt
                              ? dayjs(session.lastActivityAt).fromNow()
                              : 'Unknown'}
                          </Text>
                        </Table.Td>
                        <Table.Td>
                          <Text size="sm">
                            {dayjs(session.expiresAt).fromNow()}
                          </Text>
                        </Table.Td>
                        <Table.Td>
                          <Tooltip label="Revoke session">
                            <ActionIcon
                              variant="subtle"
                              color="red"
                              onClick={() => handleRevokeSession(session)}
                              loading={revokeSessionMutation.isPending}
                            >
                              <IconTrash size={18} />
                            </ActionIcon>
                          </Tooltip>
                        </Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              </Table.ScrollContainer>
            </Stack>
          </Card>
        ) : (
          <Card shadow="sm" radius="lg" withBorder>
            <Text c="dimmed" ta="center" py="xl">
              No other active sessions found
            </Text>
          </Card>
        )}
      </Stack>

      {/* Revoke Single Session Modal */}
      <Modal
        opened={revokeModalOpened}
        onClose={closeRevokeModal}
        title="Revoke Session"
        centered
        radius="lg"
      >
        <Stack gap="md">
          <Text>
            Are you sure you want to revoke this session? The device will need to sign in again.
          </Text>
          {selectedSession && (
            <Card withBorder radius="md" p="sm">
              <Group gap="sm">
                {getDeviceIcon(selectedSession.userAgent)}
                <div>
                  <Text size="sm" fw={500}>
                    {selectedSession.deviceName || 'Unknown Device'}
                  </Text>
                  <Text size="xs" c="dimmed">
                    {selectedSession.ipAddress || 'Unknown location'}
                  </Text>
                </div>
              </Group>
            </Card>
          )}
          <Group justify="flex-end" mt="md">
            <Button variant="subtle" onClick={closeRevokeModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevoke} loading={revokeSessionMutation.isPending}>
              Revoke Session
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* Revoke All Sessions Modal */}
      <Modal
        opened={revokeAllModalOpened}
        onClose={closeRevokeAllModal}
        title="Revoke All Other Sessions"
        centered
        radius="lg"
      >
        <Stack gap="md">
          <Text>
            Are you sure you want to revoke all other sessions? All other devices will need to sign in again.
          </Text>
          <Text size="sm" c="dimmed">
            This will affect {otherSessions.length} session{otherSessions.length !== 1 ? 's' : ''}.
          </Text>
          <Group justify="flex-end" mt="md">
            <Button variant="subtle" onClick={closeRevokeAllModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevokeAll} loading={revokeAllMutation.isPending}>
              Revoke All
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Container>
  );
}
