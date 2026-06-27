import { ActionIcon, Badge, Button, Card, DataPreview, Loader, Text } from "@octofhir/ui-kit";
import {
  ArrowLeft,
  Info as CircleInfo,
  Key,
  Pencil,
  User as Person,
  Shield,
  ShieldCheck,
  Smartphone,
  Trash2 as TrashBin,
} from "lucide-react";
import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  formatUserDateTime,
  formatUserRelativeTime,
  getUserRoleView,
  getUserSessionBrowser,
  getUserStatusView,
} from "@/entities/user-account";
import type { UserSession } from "@/shared/api/types";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { useRevokeSession, useUser, useUserSessions } from "../lib/useUsers";
import { EditUserModal } from "./EditUserModal";
import classes from "./UserDetailPage.module.css";

export function UserDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data: user, isLoading: userLoading } = useUser(id ?? null);
  const { data: sessions, isLoading: sessionsLoading } = useUserSessions(id ?? null);
  const revokeSession = useRevokeSession();
  const [editModalOpened, setEditModalOpened] = useState(false);

  if (userLoading) {
    return (
      <div className={classes.pageState}>
        <Loader size="lg" />
        <Text c="dimmed">Loading user...</Text>
      </div>
    );
  }

  if (!user) {
    return (
      <div className={classes.pageState}>
        <Text c="dimmed">User not found</Text>
        <Button variant="light" onClick={() => navigate("/admin/users")}>
          Back to Users
        </Button>
      </div>
    );
  }

  const handleRevokeSession = (session: UserSession) => {
    if (id) {
      revokeSession.mutate({ userId: id, sessionId: session.id });
    }
  };

  const activeSessions = sessions?.length ?? 0;
  const statusView = getUserStatusView(user);

  return (
    <WorkspacePageLayout
      title={user.name || user.username}
      description={user.email || user.username}
      meta={
        <div className={classes.metaBadges}>
          <Badge color={statusView.color} variant="light">
            {statusView.label}
          </Badge>
          {user.mfaEnabled ? (
            <Badge color="blue" variant="light" leftSection={<ShieldCheck size={12} />}>
              MFA Enabled
            </Badge>
          ) : null}
          <Badge color="gray" variant="light">
            {user.roles?.length ?? 0} roles
          </Badge>
          <Badge color="gray" variant="light">
            {activeSessions} sessions
          </Badge>
          <Badge color="gray" variant="light">
            Last login {formatUserRelativeTime(user.lastLogin)}
          </Badge>
        </div>
      }
      actions={
        <div className={classes.actions}>
          <Button
            variant="subtle"
            leftSection={<ArrowLeft size={16} />}
            onClick={() => navigate("/auth/users")}
          >
            Back
          </Button>
          <Button
            variant="light"
            leftSection={<Pencil size={16} />}
            onClick={() => setEditModalOpened(true)}
          >
            Edit
          </Button>
          <Button variant="light" leftSection={<Key size={16} />}>
            Reset Password
          </Button>
          <ActionIcon variant="light" color="red" size="lg">
            <TrashBin size={16} />
          </ActionIcon>
        </div>
      }
    >
      <div className={classes.contentGrid}>
        <div className={classes.detailColumn}>
          <Card className={classes.sectionCard}>
            <Text as="h2" variant="subheader-3" className={classes.sectionTitle}>
              <CircleInfo size={18} />
              Profile Information
            </Text>
            <div className={classes.infoGrid}>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>Username</Text>
                <Text className={classes.infoValue}>{user.username}</Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>Email</Text>
                <Text className={classes.infoValue}>{user.email || "Not set"}</Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>Full Name</Text>
                <Text className={classes.infoValue}>{user.name || "Not set"}</Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>FHIR User</Text>
                <Text className={classes.infoValue}>
                  {user.fhirUser?.reference || "Not linked"}
                </Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>MFA Status</Text>
                <Text className={classes.infoValue}>
                  {user.mfaEnabled ? "Enabled" : "Disabled"}
                </Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>Created</Text>
                <Text className={classes.infoValue}>{formatUserDateTime(user.createdAt)}</Text>
              </div>
              <div className={classes.infoItem}>
                <Text className={classes.infoLabel}>Last Updated</Text>
                <Text className={classes.infoValue}>{formatUserDateTime(user.updatedAt)}</Text>
              </div>
            </div>
          </Card>

          {/* Roles */}
          <Card className={classes.sectionCard}>
            <Text as="h2" variant="subheader-3" className={classes.sectionTitle}>
              <Shield size={18} />
              Assigned Roles
            </Text>
            {user.roles && user.roles.length > 0 ? (
              <div className={classes.rolesList}>
                {user.roles.map((role) => {
                  const roleView = getUserRoleView(role);

                  return (
                    <div key={role} className={classes.roleItem}>
                      <Text className={classes.roleName}>{roleView.role}</Text>
                      <Badge
                        size="sm"
                        variant={roleView.theme === "danger" ? "filled" : "light"}
                        color={roleView.theme === "danger" ? "red" : "blue"}
                      >
                        {roleView.theme === "danger" ? "System" : "Custom"}
                      </Badge>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className={classes.emptyState}>
                <Text size="sm" c="dimmed">
                  No roles assigned
                </Text>
              </div>
            )}
          </Card>
        </div>

        <div className={classes.detailColumn}>
          <Card className={classes.sectionCard}>
            <Text as="h2" variant="subheader-3" className={classes.sectionTitle}>
              <Smartphone size={18} />
              Active Sessions
            </Text>
            {sessionsLoading ? (
              <div className={classes.cardState}>
                <Loader size="sm" />
              </div>
            ) : sessions && sessions.length > 0 ? (
              <div className={classes.sessionsList}>
                {sessions.map((session) => (
                  <div key={session.id} className={classes.sessionItem}>
                    <div className={classes.sessionInfo}>
                      <Text className={classes.sessionDevice}>
                        {session.userAgent
                          ? getUserSessionBrowser(session)
                          : session.clientName || "Unknown device"}
                        {session.isCurrent && (
                          <Badge size="xs" ml="xs" color="green">
                            Current
                          </Badge>
                        )}
                      </Text>
                      <Text className={classes.sessionMeta}>
                        {session.ipAddress || "Unknown IP"} · Last active{" "}
                        {formatUserRelativeTime(session.lastActivity)}
                      </Text>
                    </div>
                    {!session.isCurrent && (
                      <Button
                        size="xs"
                        variant="light"
                        color="red"
                        onClick={() => handleRevokeSession(session)}
                        loading={revokeSession.isPending}
                      >
                        Revoke
                      </Button>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className={classes.emptyState}>
                <Text size="sm" c="dimmed">
                  No active sessions
                </Text>
              </div>
            )}
          </Card>

          <Card className={classes.sectionCard}>
            <Text as="h2" variant="subheader-3" className={classes.sectionTitle}>
              <Person size={18} />
              Linked Identities
            </Text>
            {user.identity && user.identity.length > 0 ? (
              <DataPreview
                columns={[
                  { id: "provider", label: "Provider" },
                  { id: "email", label: "Email" },
                  { id: "linked", label: "Linked", width: 130 },
                ]}
                rows={user.identity.map((identity) => ({
                  provider: (
                    <Text size="sm" className={classes.tableText}>
                      {identity.provider?.display || "Unknown"}
                    </Text>
                  ),
                  email: (
                    <Text size="sm" className={classes.tableText}>
                      {identity.email || "-"}
                    </Text>
                  ),
                  linked: <Text size="sm">{formatUserRelativeTime(identity.linkedAt)}</Text>,
                }))}
                getRowKey={(_row, index) => user.identity?.[index]?.externalId ?? `${index}`}
              />
            ) : (
              <div className={classes.emptyState}>
                <Text size="sm" c="dimmed">
                  No linked identities
                </Text>
              </div>
            )}
          </Card>
        </div>
      </div>

      <EditUserModal
        user={user}
        opened={editModalOpened}
        onClose={() => setEditModalOpened(false)}
      />
    </WorkspacePageLayout>
  );
}
