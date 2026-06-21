import { Badge, type BadgeProps } from "../Badge";

export type StatusTone = "neutral" | "info" | "success" | "warning" | "danger";

export interface StatusBadgeProps extends Omit<BadgeProps, "theme" | "color"> {
    tone?: StatusTone;
}

const themeByTone: Record<StatusTone, BadgeProps["theme"]> = {
    neutral: "unknown",
    info: "info",
    success: "success",
    warning: "warning",
    danger: "danger",
};

export function StatusBadge({ tone = "neutral", size = "sm", ...props }: StatusBadgeProps) {
    return <Badge theme={themeByTone[tone]} size={size} {...props} />;
}
