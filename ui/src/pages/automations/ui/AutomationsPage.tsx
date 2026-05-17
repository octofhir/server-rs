import { useState, useMemo } from "react";
import {
  Alert,
  Button,
  TextInput,
  Select,
  Table,
  ActionIcon,
  Text,
  Tooltip,
  Loader,
  Badge,
} from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { modals, notifications } from "@octofhir/ui-kit";
import { useNavigate } from "react-router-dom";
import { isAutomationFeatureUnavailableError } from "@/shared/api/automationsApi";
import {
  Plus,
  Magnifier,
  Pencil,
  TrashBin,
  Play,
  Rocket,
  Clock,
  Thunderbolt,
  HandPointRight,
  CircleExclamation,
} from "@gravity-ui/icons";
import { useAutomations, useDeleteAutomation, useDeployAutomation } from "../lib/useAutomations";
import { AutomationStatusBadge } from "./AutomationStatusBadge";
import { CreateAutomationModal } from "./CreateAutomationModal";
import type { Automation, AutomationStatus, AutomationTriggerType } from "@/shared/api/types";
import classes from "./AutomationsPage.module.css";

type StatusFilter = AutomationStatus | "";

const statusOptions: Array<{ value: StatusFilter; label: string }> = [
  { value: "", label: "All statuses" },
  { value: "active", label: "Active" },
  { value: "inactive", label: "Inactive" },
  { value: "error", label: "Error" },
];

function isStatusFilter(value: string | null): value is StatusFilter {
  return value === "" || value === "active" || value === "inactive" || value === "error";
}

const triggerTypeIcons: Record<AutomationTriggerType, React.ReactNode> = {
  resource_event: <Thunderbolt size={14} />,
  cron: <Clock size={14} />,
  manual: <HandPointRight size={14} />,
};

const triggerTypeLabels: Record<AutomationTriggerType, string> = {
  resource_event: "Resource Event",
  cron: "Scheduled",
  manual: "Manual",
};

export function AutomationsPage() {
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("");
  const [createModalOpen, setCreateModalOpen] = useState(false);

  const { data, isLoading, error } = useAutomations({
    status: statusFilter || undefined,
    name: search || undefined,
  });

  const deleteMutation = useDeleteAutomation();
  const deployMutation = useDeployAutomation();
  const isFeatureUnavailable = isAutomationFeatureUnavailableError(error);

  // Filter automations by search (client-side for responsiveness)
  const filteredAutomations = useMemo(() => {
    if (!data?.automations) return [];
    if (!search) return data.automations;

    const searchLower = search.toLowerCase();
    return data.automations.filter(
      (a) =>
        a.name.toLowerCase().includes(searchLower) ||
        a.description?.toLowerCase().includes(searchLower),
    );
  }, [data?.automations, search]);

  const handleEdit = (automation: Automation) => {
    navigate(`/automations/${automation.id}`);
  };

  const handleDelete = (automation: Automation) => {
    modals.openConfirmModal({
      title: "Delete Automation",
      children: (
        <Text size="sm">
          Are you sure you want to delete <strong>{automation.name}</strong>? This action cannot be
          undone.
        </Text>
      ),
      labels: { confirm: "Delete", cancel: "Cancel" },
      confirmProps: { color: "red" },
      onConfirm: async () => {
        try {
          await deleteMutation.mutateAsync(automation.id);
          notifications.show({
            title: "Deleted",
            message: `Automation "${automation.name}" has been deleted`,
            color: "green",
          });
        } catch (error) {
          notifications.show({
            title: "Error",
            message: error instanceof Error ? error.message : "Failed to delete automation",
            color: "red",
          });
        }
      },
    });
  };

  const handleDeploy = async (automation: Automation) => {
    try {
      await deployMutation.mutateAsync(automation.id);
      notifications.show({
        title: "Deployed",
        message: `Automation "${automation.name}" has been deployed and activated`,
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Deploy Failed",
        message: error instanceof Error ? error.message : "Failed to deploy automation",
        color: "red",
      });
    }
  };

  const formatTriggers = (automation: Automation) => {
    if (!automation.triggers || automation.triggers.length === 0) {
      return <Text c="dimmed" size="sm">No triggers</Text>;
    }

    return (
      <div className={classes.triggerBadges}>
        {automation.triggers.map((trigger) => (
          <Tooltip
            key={trigger.id}
            label={
              trigger.trigger_type === "resource_event"
                ? `${trigger.resource_type}: ${trigger.event_types?.join(", ")}`
                : trigger.trigger_type === "cron"
                  ? trigger.cron_expression
                  : "Manual execution"
            }
          >
            <Badge
              size="xs"
              variant="light"
              leftSection={triggerTypeIcons[trigger.trigger_type]}
            >
              {trigger.trigger_type === "resource_event"
                ? trigger.resource_type
                : triggerTypeLabels[trigger.trigger_type]}
            </Badge>
          </Tooltip>
        ))}
      </div>
    );
  };

  const formatDate = (dateString: string) => {
    const date = new Date(dateString);
    return date.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const formatRelativeTime = (dateString: string) => {
    const date = new Date(dateString);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / (1000 * 60));
    const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

    if (diffMins < 1) return "just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;
    return formatDate(dateString);
  };

  const formatLastRun = (automation: Automation) => {
    const stats = automation.execution_stats;
    if (!stats || !stats.last_execution_status) {
      return <Text c="dimmed" size="sm">—</Text>;
    }

    const statusColor = stats.last_execution_status === "completed" ? "green" :
                        stats.last_execution_status === "failed" ? "red" : "blue";
    const statusLabel = stats.last_execution_status === "completed" ? "Success" :
                        stats.last_execution_status === "failed" ? "Failed" : "Running";

    return (
      <div className={classes.lastRun}>
        <Tooltip
          label={stats.last_error || `Last run: ${formatDate(stats.last_execution_at || "")}`}
          multiline
          w={300}
          withArrow
        >
          <Badge color={statusColor} size="sm" variant="light">
            {statusLabel}
          </Badge>
        </Tooltip>
        {stats.last_execution_at && (
          <Text size="xs" c="dimmed">
            {formatRelativeTime(stats.last_execution_at)}
          </Text>
        )}
        {stats.failure_count_24h > 0 && (
          <Tooltip label={`${stats.failure_count_24h} failed in last 24h`}>
            <Badge color="red" size="xs" variant="filled">
              {stats.failure_count_24h}✕
            </Badge>
          </Tooltip>
        )}
      </div>
    );
  };

  return (
    <WorkspacePageLayout
      title="Automations"
      description="Create, deploy, and test event-driven automation workflows"
      className="page-enter"
      actions={
        isFeatureUnavailable ? null : (
          <Button leftSection={<Plus size={16} />} onClick={() => setCreateModalOpen(true)}>
            New Automation
          </Button>
        )
      }
      toolbar={
        isFeatureUnavailable ? undefined : (
          <div className={classes.toolbar}>
            <TextInput
              placeholder="Search automations..."
              leftSection={<Magnifier size={16} />}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className={classes.searchInput}
            />
            <Select
              data={statusOptions}
              value={statusFilter}
              onChange={(value) => {
                if (isStatusFilter(value)) {
                  setStatusFilter(value);
                }
              }}
              placeholder="Filter by status"
              clearable
              className={classes.statusSelect}
            />
          </div>
        )
      }
    >
      <div className={classes.tablePanel}>
        {isFeatureUnavailable ? (
          <Alert
            theme="warning"
            icon={<CircleExclamation size={16} />}
            title="Automations are disabled"
            className={classes.unavailableAlert}
          >
            <Text variant="body-2">
              The backend did not expose the automation API. Enable automations in server
              configuration to create, deploy, and test workflows.
            </Text>
          </Alert>
        ) : error ? (
          <Alert
            theme="danger"
            icon={<CircleExclamation size={16} />}
            title="Automation API is unavailable"
          >
            <Text variant="body-2">
              {error instanceof Error ? error.message : "Failed to load automations"}
            </Text>
          </Alert>
        ) : isLoading ? (
          <div className={classes.statePanel}>
            <Loader />
          </div>
        ) : filteredAutomations.length === 0 ? (
          <div className={classes.emptyState}>
              <Text c="dimmed">No automations found</Text>
              <Button
                variant="light"
                leftSection={<Plus size={16} />}
                onClick={() => setCreateModalOpen(true)}
              >
                Create your first automation
              </Button>
          </div>
        ) : (
          <Table.ScrollContainer minWidth={940}>
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>Name</Table.Th>
                  <Table.Th>Status</Table.Th>
                  <Table.Th>Last Run</Table.Th>
                  <Table.Th>Triggers</Table.Th>
                  <Table.Th>Updated</Table.Th>
                  <Table.Th w={140}>Actions</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {filteredAutomations.map((automation) => (
                  <Table.Tr key={automation.id}>
                    <Table.Td>
                      <div className={classes.nameCell}>
                        <Text fw={500}>{automation.name}</Text>
                        {automation.description && (
                          <Text size="xs" c="dimmed" className={classes.truncateText}>
                            {automation.description}
                          </Text>
                        )}
                      </div>
                    </Table.Td>
                    <Table.Td>
                      <AutomationStatusBadge status={automation.status} />
                    </Table.Td>
                    <Table.Td>{formatLastRun(automation)}</Table.Td>
                    <Table.Td>{formatTriggers(automation)}</Table.Td>
                    <Table.Td>
                      <Text size="sm" c="dimmed">
                        {formatDate(automation.updated_at)}
                      </Text>
                    </Table.Td>
                    <Table.Td>
                      <div className={classes.rowActions}>
                        <Tooltip label="Edit">
                          <ActionIcon
                            variant="subtle"
                            color="gray"
                            onClick={() => handleEdit(automation)}
                          >
                            <Pencil size={16} />
                          </ActionIcon>
                        </Tooltip>
                        <Tooltip label={automation.status === "active" ? "Re-deploy" : "Deploy"}>
                          <ActionIcon
                            variant="subtle"
                            color="blue"
                            onClick={() => handleDeploy(automation)}
                            loading={deployMutation.isPending}
                          >
                            <Rocket size={16} />
                          </ActionIcon>
                        </Tooltip>
                        <Tooltip label="Test">
                          <ActionIcon
                            variant="subtle"
                            color="green"
                            onClick={() => navigate(`/automations/${automation.id}?tab=playground`)}
                          >
                            <Play size={16} />
                          </ActionIcon>
                        </Tooltip>
                        <Tooltip label="Delete">
                          <ActionIcon
                            variant="subtle"
                            color="red"
                            onClick={() => handleDelete(automation)}
                          >
                            <TrashBin size={16} />
                          </ActionIcon>
                        </Tooltip>
                      </div>
                    </Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
        )}
      </div>

      {/* Total count */}
      {data?.total !== undefined && (
        <Text size="sm" c="dimmed">
          {data.total} automation{data.total !== 1 ? "s" : ""} total
        </Text>
      )}

      {/* Create Modal */}
      <CreateAutomationModal opened={createModalOpen} onClose={() => setCreateModalOpen(false)} />
    </WorkspacePageLayout>
  );
}
