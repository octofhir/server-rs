import type { ComponentType, ReactNode } from "react";
import { Card, Text, ThemeIcon, Button, Box } from "../../shared/ui";
import classes from "./DashboardCards.module.css";

type ActionCardTone = "primary" | "positive" | "warning" | "danger" | "neutral";

export interface ActionCardProps {
    title: string;
    description: string;
    icon: ComponentType<{ size?: number | string }>;
    color?: ActionCardTone;
    onClick?: () => void;
    buttonText?: string;
    buttonIcon?: ReactNode;
}

export function ActionCard({
    title,
    description,
    icon: Icon,
    color = "primary",
    onClick,
    buttonText = "Open Tool",
    buttonIcon,
}: ActionCardProps) {
    return (
        <Card
            withBorder
            padding="xl"
            radius={8}
            className={classes.actionCard}
            data-clickable={onClick ? "true" : "false"}
            onClick={onClick}
        >
            <Box className={classes.actionIcon}>
                <ThemeIcon
                    variant="light"
                    color={color}
                    size="xl"
                >
                    <Icon size={24} />
                </ThemeIcon>
            </Box>
            <Box className={classes.actionTitle}>
                <Text variant="subheader-1">
                    {title}
                </Text>
            </Box>
            <Box className={classes.actionDescription}>
                <Text variant="body-1" color="secondary">
                    {description}
                </Text>
            </Box>
            <Button
                variant="subtle"
                size="sm"
                rightSection={buttonIcon}
                className={classes.actionButton}
            >
                {buttonText}
            </Button>
        </Card>
    );
}
