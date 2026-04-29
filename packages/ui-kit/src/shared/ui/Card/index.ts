import React, { forwardRef } from "react";
import {
    Card as GravityCard,
    type CardProps as GravityCardProps,
    type CardView,
} from "@gravity-ui/uikit";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

export interface CardProps extends Omit<GravityCardProps, "view" | "style"> {
    view?: CardView;
    withBorder?: boolean;
    radius?: string | number;
    shadow?: string;
    padding?: number | string;
    p?: number | string;
    bg?: string;
    ta?: React.CSSProperties["textAlign"];
    style?: React.CSSProperties;
    w?: number | string;
    h?: number | string;
    px?: number | string;
    py?: number | string;
    pt?: number | string;
    pb?: number | string;
    pl?: number | string;
    pr?: number | string;
    m?: number | string;
    mx?: number | string;
    my?: number | string;
    mt?: number | string;
    mb?: number | string;
    ml?: number | string;
    mr?: number | string;
}

export const Card = forwardRef<HTMLDivElement, CardProps>(
    (
        {
            withBorder,
            radius,
            shadow,
            padding,
            p,
            bg,
            ta,
            view,
            type = "container",
            style,
            ...props
        },
        ref,
    ) => {
        const cleaned = cleanLayoutProps(props);
        const mergedStyle: React.CSSProperties = {
            ...getSpacingStyles({ ...props, p: p ?? padding }),
            ...(bg ? { background: bg } : {}),
            ...(ta ? { textAlign: ta } : {}),
            ...(radius !== undefined ? { borderRadius: radius } : {}),
            ...(shadow ? { boxShadow: "var(--g-box-shadow-light)" } : {}),
            ...style,
        };

        return React.createElement(GravityCard, {
            ref,
            type,
            view: view ?? (withBorder ? "outlined" : "filled"),
            style: mergedStyle,
            ...cleaned,
        });
    },
);

Card.displayName = "Card";
