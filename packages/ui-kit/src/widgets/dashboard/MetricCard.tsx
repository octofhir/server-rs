import React from "react";
import { Card, Text, Group, ThemeIcon, Box, Loader } from "../../shared/ui";

export interface MetricCardProps {
    title: string;
    value: string | number | React.ReactNode;
    icon: React.ComponentType<any>;
    iconColor?: string;
    description?: string;
    isLoading?: boolean;
    gradientColor?: "primary" | "warning" | "danger" | "success" | "info" | string;
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
            radius="lg"
            style={{
                position: "relative",
                overflow: "hidden",
                height: "100%",
            }}
        >
            <Box
                style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    right: 0,
                    height: 4,
                    background: topBarColor,
                }}
            />
            <Group justify="space-between" align="flex-start" mb="md">
                <div>
                    <Box mb={4}>
                        <Text variant="caption-2" color="secondary" style={{ letterSpacing: "0.05em", textTransform: "uppercase", fontWeight: 600 }}>
                            {title}
                        </Text>
                    </Box>
                    {isLoading ? (
                        <Loader size="s" />
                    ) : (
                        <Text variant="header-2" style={{ color: valueTextColor, letterSpacing: "-0.02em", display: "block" }}>
                            {value}
                        </Text>
                    )}
                </div>
                <ThemeIcon view="light" color={iconColor as any} size="xl">
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
