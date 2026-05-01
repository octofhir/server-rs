import type { ComponentType, CSSProperties, ReactNode } from "react";
import { Card, Text, Group, ThemeIcon, Box, Loader } from "../../shared/ui";
import classes from "./DashboardCards.module.css";

type MetricTone = "primary" | "warning" | "danger" | "success" | "info" | "octo-brand" | string;
type MetricIconTone = "primary" | "positive" | "warning" | "danger" | "neutral";

export interface MetricCardProps {
    title: string;
    value: string | number | ReactNode;
    icon: ComponentType<{ size?: number | string }>;
    iconColor?: MetricIconTone;
    description?: string;
    isLoading?: boolean;
    gradientColor?: MetricTone;
}

const gradientColors: Record<string, string> = {
    primary: "var(--g-color-base-brand)",
    warning: "var(--g-color-base-warning-medium)",
    danger: "var(--g-color-base-danger-medium)",
    success: "var(--g-color-base-positive-medium)",
    info: "var(--g-color-base-info-medium)",
    "octo-brand": "var(--octo-brand-gradient, var(--g-color-base-brand))",
};

const textColors: Record<string, string> = {
    primary: "var(--g-color-text-brand)",
    warning: "var(--g-color-text-warning)",
    danger: "var(--g-color-text-danger)",
    success: "var(--g-color-text-positive)",
    info: "var(--g-color-text-info)",
    "octo-brand": "var(--g-color-text-primary)",
};

export function MetricCard({
    title,
    value,
    icon: Icon,
    iconColor = "primary",
    description,
    isLoading = false,
    gradientColor = "primary",
}: MetricCardProps) {
    const topBarColor = gradientColors[gradientColor] || gradientColor;
    const valueTextColor = textColors[gradientColor] || "var(--g-color-text-primary)";

    return (
        <Card
            withBorder
            padding="xl"
            radius={8}
            className={classes.card}
            style={
                {
                    "--metric-accent-color": topBarColor,
                    "--metric-value-color": valueTextColor,
                } as CSSProperties
            }
        >
            <Box className={classes.topBar} />
            <Group className={classes.metricHeader}>
                <div>
                    <Box mb={4}>
                        <Text variant="caption-2" color="secondary" className={classes.metricTitle}>
                            {title}
                        </Text>
                    </Box>
                    {isLoading ? (
                        <Loader size="s" />
                    ) : (
                        <Text variant="header-2" className={classes.metricValue}>
                            {value}
                        </Text>
                    )}
                </div>
                <ThemeIcon view="light" color={iconColor} size="xl">
                    <Icon size={24} />
                </ThemeIcon>
            </Group>
            {description && (
                <Text variant="caption-1" color="secondary">
                    {description}
                </Text>
            )}
        </Card>
    );
}
