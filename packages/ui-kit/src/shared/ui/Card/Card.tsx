import { Card as MantineCard, type CardProps, type ElementProps } from "@mantine/core";
import classes from "./Card.module.css";

export interface OctoCardProps extends CardProps, ElementProps<"div"> { }

export function Card({ onClick, className, ...others }: OctoCardProps) {
    return (
        <MantineCard
            onClick={onClick}
            data-clickable={onClick ? "" : undefined}
            className={`${classes.root} ${className || ""}`}
            {...others}
        />
    );
}

export type { CardProps };
