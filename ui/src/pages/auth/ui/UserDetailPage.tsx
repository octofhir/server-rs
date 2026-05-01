import { useState } from "react";
import { Stack, Title, Text, Group, Badge, Table, Loader } from "@/shared/ui";
import { useParams, useNavigate } from "react-router-dom";
import {
	ArrowLeft,
	Pencil,
	Key,
	TrashBin,
	Shield,
	Smartphone,
	Person,
	CircleInfo,
	ShieldCheck,
} from "@gravity-ui/icons";
import { Card } from "@/shared/ui/Card/Card";
import { Button } from "@/shared/ui/Button/Button";
import { ActionIcon } from "@/shared/ui/ActionIcon/ActionIcon";
import {
	formatUserDateTime,
	formatUserRelativeTime,
	getUserInitials,
	getUserRoleView,
	getUserSessionBrowser,
	getUserStatusView,
} from "@/entities/user-account";
import { useUser, useUserSessions, useRevokeSession } from "../lib/useUsers";
import type { UserSession } from "@/shared/api/types";
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
			<Stack align="center" justify="center" style={{ flex: 1 }}>
				<Loader size="lg" />
				<Text c="dimmed">Loading user...</Text>
			</Stack>
		);
	}

	if (!user) {
		return (
			<Stack align="center" justify="center" style={{ flex: 1 }}>
				<Text c="dimmed">User not found</Text>
				<Button variant="light" onClick={() => navigate("/admin/users")}>
					Back to Users
				</Button>
			</Stack>
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
		<Stack gap="md" className={classes.pageRoot}>
			{/* Back button */}
			<Group>
				<Button
					variant="subtle"
					leftSection={<ArrowLeft size={16} />}
					onClick={() => navigate("/auth/users")}
				>
					Back to Users
				</Button>
			</Group>

			{/* Profile Header Card */}
			<Card className={classes.headerCard}>
				<div className={classes.profileHeader}>
					<div className={classes.avatar}>{getUserInitials(user)}</div>
					<div className={classes.profileInfo}>
						<Text className={classes.userName}>{user.name || user.username}</Text>
						<Text className={classes.userEmail}>{user.email || user.username}</Text>
						<Group gap="xs" mt="xs">
							<Badge color={statusView.color} variant="light">
								{statusView.label}
							</Badge>
							{user.mfaEnabled && (
								<Badge color="blue" variant="light" leftSection={<ShieldCheck size={12} />}>
									MFA Enabled
								</Badge>
							)}
						</Group>
					</div>
					<div className={classes.profileActions}>
						<Button variant="light" leftSection={<Pencil size={16} />} onClick={() => setEditModalOpened(true)}>
							Edit
						</Button>
						<Button variant="light" leftSection={<Key size={16} />}>
							Reset Password
						</Button>
						<ActionIcon variant="light" color="red" size="lg">
							<TrashBin size={16} />
						</ActionIcon>
					</div>
				</div>

				{/* Stats */}
				<div className={classes.statsGrid}>
					<div className={classes.statItem}>
						<Text className={classes.statValue}>{user.roles?.length ?? 0}</Text>
						<Text className={classes.statLabel}>Roles</Text>
					</div>
					<div className={classes.statItem}>
						<Text className={classes.statValue}>{activeSessions}</Text>
						<Text className={classes.statLabel}>Active Sessions</Text>
					</div>
					<div className={classes.statItem}>
						<Text className={classes.statValue}>{user.identity?.length ?? 0}</Text>
						<Text className={classes.statLabel}>Linked Identities</Text>
					</div>
					<div className={classes.statItem}>
						<Text className={classes.statValue}>{formatUserRelativeTime(user.lastLogin)}</Text>
						<Text className={classes.statLabel}>Last Login</Text>
					</div>
				</div>
			</Card>

			{/* Content Grid */}
			<div className={classes.contentGrid}>
				{/* Left Column */}
				<Stack gap="md">
					{/* Profile Info */}
					<Card className={classes.sectionCard}>
						<Title order={4} className={classes.sectionTitle}>
							<CircleInfo size={18} />
							Profile Information
						</Title>
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
						<Title order={4} className={classes.sectionTitle}>
							<Shield size={18} />
							Assigned Roles
						</Title>
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
				</Stack>

				{/* Right Column */}
				<Stack gap="md">
					{/* Active Sessions */}
					<Card className={classes.sectionCard}>
						<Title order={4} className={classes.sectionTitle}>
							<Smartphone size={18} />
							Active Sessions
						</Title>
						{sessionsLoading ? (
							<Stack align="center" py="md">
								<Loader size="sm" />
							</Stack>
						) : sessions && sessions.length > 0 ? (
							<Stack gap="xs">
								{sessions.map((session) => (
									<div key={session.id} className={classes.sessionItem}>
										<div className={classes.sessionInfo}>
											<Text className={classes.sessionDevice}>
												{getUserSessionBrowser(session)}
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
							</Stack>
						) : (
							<div className={classes.emptyState}>
								<Text size="sm" c="dimmed">
									No active sessions
								</Text>
							</div>
						)}
					</Card>

					{/* Linked Identities */}
					<Card className={classes.sectionCard}>
						<Title order={4} className={classes.sectionTitle}>
							<Person size={18} />
							Linked Identities
						</Title>
						{user.identity && user.identity.length > 0 ? (
							<Table>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>Provider</Table.Th>
										<Table.Th>Email</Table.Th>
										<Table.Th>Linked</Table.Th>
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{user.identity.map((identity) => (
										<Table.Tr key={identity.externalId}>
											<Table.Td>{identity.provider?.display || "Unknown"}</Table.Td>
											<Table.Td>{identity.email || "-"}</Table.Td>
											<Table.Td>{formatUserRelativeTime(identity.linkedAt)}</Table.Td>
										</Table.Tr>
									))}
								</Table.Tbody>
							</Table>
						) : (
							<div className={classes.emptyState}>
								<Text size="sm" c="dimmed">
									No linked identities
								</Text>
							</div>
						)}
					</Card>
				</Stack>
			</div>

			{/* Edit User Modal */}
			<EditUserModal user={user} opened={editModalOpened} onClose={() => setEditModalOpened(false)} />
		</Stack>
	);
}
