import type { ComponentProps } from "react";
import { Card as KitCard } from "@octofhir/ui-kit";
import classes from "./Card.module.css";

export type AppCardProps = ComponentProps<typeof KitCard>;

export function Card({ onClick, className, ...others }: AppCardProps) {
    return (
        <KitCard
            onClick={onClick}
            data-clickable={onClick ? "" : undefined}
            className={`${classes.root} ${className || ""}`}
            {...others}
        />
    );
}
