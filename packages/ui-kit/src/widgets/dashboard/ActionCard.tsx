import React from "react";
import { Card, Text, ThemeIcon, Button, Box } from "../../shared/ui";

export interface ActionCardProps {
    title: string;
    description: string;
    icon: React.ComponentType<any>;
    color?: string;
    onClick?: () => void;
    buttonText?: string;
    buttonIcon?: React.ReactNode;
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
            radius="lg"
            className="octo-action-card"
            style={{
                cursor: onClick ? "pointer" : "default",
                transition: "all 0.2s ease",
            }}
            onClick={onClick}
        >
            <Box mb="lg">
                <ThemeIcon
                    view="light"
                    color={color as any}
                    size="xl"
                    style={{
                        boxShadow: "0 8px 16px var(--g-color-base-brand-light-hover)",
                    }}
                >
                    <Icon size={24} />
                </ThemeIcon>
            </Box>
            <Box mb="xs">
                <Text variant="subheader-1" style={{ letterSpacing: "-0.01em" }}>
                    {title}
                </Text>
            </Box>
            <Box mb="md">
                <Text variant="body-1" color="secondary" style={{ lineHeight: 1.5 }}>
                    {description}
                </Text>
            </Box>
            <Button
                view="flat"
                size="s"
                rightSection={buttonIcon}
                style={{ width: "fit-content", padding: 0 }}
            >
                {buttonText}
            </Button>
            <style dangerouslySetInnerHTML={{
                __html: `
                .octo-action-card:hover {
                    transform: translateY(-4px);
                    box-shadow: var(--octo-shadow-md);
                    border-color: var(--octo-accent-primary);
                }
            `}} />
        </Card>
    );
}
