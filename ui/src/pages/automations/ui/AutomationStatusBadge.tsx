import { Badge, type BadgeProps } from "@mantine/core";
import type { AutomationStatus } from "@/shared/api/types";

interface AutomationStatusBadgeProps extends Omit<BadgeProps, "color"> {
  status: AutomationStatus;
}

const statusConfig: Record<AutomationStatus, { color: string; label: string }> = {
  active: { color: "green", label: "Active" },
  inactive: { color: "gray", label: "Inactive" },
  error: { color: "red", label: "Error" },
};

export function AutomationStatusBadge({ status, ...props }: AutomationStatusBadgeProps) {
  const config = statusConfig[status] || statusConfig.inactive;

  return (
    <Badge color={config.color} variant="light" size="sm" {...props}>
      {config.label}
    </Badge>
  );
}
