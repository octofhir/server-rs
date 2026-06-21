import { useDisclosure } from "@octofhir/ui-kit";
import {
  ActionIcon,
  Badge,
  Button,
  Card,
  DataPreview,
  EmptyState,
  KeyValueList,
  Modal,
  SectionPanel,
  Text,
  Tooltip,
} from "@octofhir/ui-kit";
import { useAuth } from "@/shared/api/hooks/useAuth";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
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
import classes from './SessionsPage.module.css';

function DeviceIcon({ kind }: { kind: SessionDeviceKind }) {
  return kind === 'mobile' ? <Smartphone size={20} /> : <Display size={20} />;
}

export function SessionsPage() {
  const { user, isLoading: userLoading } = useAuth();
  // Sessions are stored with `subject: User/{id}`; prefer the FHIR user ref,
  // fall back to the OAuth subject claim.
  const userId =
    user?.fhirUser?.startsWith('User/') ? user.fhirUser.slice('User/'.length) : user?.sub;
  const currentSessionToken = getCurrentSessionToken();

  const {
    data: sessions = [],
    isLoading: sessionsLoading,
    isError,
    error,
    refetch,
  } = useSessions(userId ?? '');
  const isLoading = userLoading || sessionsLoading;
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
          <div className={classes.actions}>
            <Button variant="subtle" leftSection={<ArrowRotateRight size={16} />} onClick={() => refetch()}>
              Refresh
            </Button>
            {otherSessions.length > 0 && (
              <Button variant="light" color="red" leftSection={<ArrowRightFromSquare size={16} />} onClick={handleRevokeAll}>
                Revoke All Other Sessions
              </Button>
            )}
          </div>
      }
    >
      <div className={classes.sections}>
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
                <div className={classes.currentSession}>
                  <div className={classes.deviceCell}>
                    <DeviceIcon kind={device.kind} />
                    <div className={classes.deviceText}>
                      <Text fw={600} className={classes.primaryText}>{device.deviceName}</Text>
                      <Text size="sm" c="dimmed" className={classes.secondaryText}>
                        {device.browser}
                      </Text>
                    </div>
                  </div>

                  <KeyValueList
                    items={[
                      { id: 'location', label: 'IP Address', value: activity.location },
                      { id: 'last-active', label: 'Last Active', value: activity.lastActive },
                      { id: 'expires', label: 'Expires', value: activity.expires },
                    ]}
                  />
                </div>
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
          {isError ? (
            <EmptyState
              title="Couldn't load sessions"
              description={error instanceof Error ? error.message : 'The session list failed to load.'}
              actions={[
                <Button key="retry" view="action" onClick={() => refetch()}>
                  Try again
                </Button>,
              ]}
            />
          ) : (
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
                        <div className={classes.deviceCell}>
                          <DeviceIcon kind={device.kind} />
                          <div className={classes.deviceText}>
                            <Text size="sm" fw={500} className={classes.primaryText}>
                              {device.deviceName}
                            </Text>
                            <Text size="xs" c="dimmed" className={classes.secondaryText}>
                              {device.browser}
                            </Text>
                          </div>
                        </div>
                      ),
                      location: <Text size="sm" className={classes.tableText}>{activity.location}</Text>,
                      lastActive: <Text size="sm" className={classes.tableText}>{activity.lastActive}</Text>,
                      expires: <Text size="sm" className={classes.tableText}>{activity.expires}</Text>,
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
          )}
        </SectionPanel>
      </div>

      <Modal
        opened={revokeModalOpened}
        onClose={closeRevokeModal}
        title="Revoke Session"
        centered
        radius="lg"
      >
        <div className={classes.modalBody}>
          <Text>
            Are you sure you want to revoke this session? The device will need to sign in again.
          </Text>
          {selectedSession && (
            <Card className={classes.sessionPreview}>
              {(() => {
                const device = getAuthSessionDeviceView(selectedSession);
                const activity = getAuthSessionActivityView(selectedSession);

                return (
                  <div className={classes.deviceCell}>
                    <DeviceIcon kind={device.kind} />
                    <div className={classes.deviceText}>
                      <Text size="sm" fw={500} className={classes.primaryText}>
                        {device.deviceName}
                      </Text>
                      <Text size="xs" c="dimmed" className={classes.secondaryText}>
                        {activity.location}
                      </Text>
                    </div>
                  </div>
                );
              })()}
            </Card>
          )}
          <div className={classes.modalActions}>
            <Button variant="subtle" onClick={closeRevokeModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevoke} loading={revokeSessionMutation.isPending}>
              Revoke Session
            </Button>
          </div>
        </div>
      </Modal>

      <Modal
        opened={revokeAllModalOpened}
        onClose={closeRevokeAllModal}
        title="Revoke All Other Sessions"
        centered
        radius="lg"
      >
        <div className={classes.modalBody}>
          <Text>
            Are you sure you want to revoke all other sessions? All other devices will need to sign in again.
          </Text>
          <Text size="sm" c="dimmed">
            This will affect {otherSessions.length} session{otherSessions.length !== 1 ? 's' : ''}.
          </Text>
          <div className={classes.modalActions}>
            <Button variant="subtle" onClick={closeRevokeAllModal}>
              Cancel
            </Button>
            <Button color="red" onClick={confirmRevokeAll} loading={revokeAllMutation.isPending}>
              Revoke All
            </Button>
          </div>
        </div>
      </Modal>
    </WorkspacePageLayout>
  );
}
