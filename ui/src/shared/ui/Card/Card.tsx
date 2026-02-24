import { Card as MantineCard, type CardProps, type ElementProps } from "@octofhir/ui-kit";
import classes from "./Card.module.css";

export interface AppCardProps extends CardProps, ElementProps<"div"> { }

export function Card({ onClick, className, ...others }: AppCardProps) {
    return (
        <MantineCard
            onClick={onClick}
            data-clickable={onClick ? "" : undefined}
            className={`${classes.root} ${className || ""}`}
            {...others}
        />
    );
}
