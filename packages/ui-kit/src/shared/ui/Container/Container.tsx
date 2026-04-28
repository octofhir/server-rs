import { forwardRef } from "react";
import type React from "react";
import { getSpacingStyles, cleanLayoutProps } from "../layout-utils";

export interface ContainerProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "color"> {
    size?: "xs" | "sm" | "md" | "lg" | "xl" | number | string;
    fluid?: boolean;
    w?: number | string;
    h?: number | string;
    p?: number | string;
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

const SIZE_MAP: Record<string, number> = {
    xs: 540,
    sm: 720,
    md: 960,
    lg: 1140,
    xl: 1320,
};

export const Container = forwardRef<HTMLDivElement, ContainerProps>(
    ({ size, fluid, style, className, ...props }, ref) => {
        const combinedStyle: React.CSSProperties = {
            width: "100%",
            marginLeft: "auto",
            marginRight: "auto",
            paddingLeft: 16,
            paddingRight: 16,
            ...getSpacingStyles(props),
            ...style,
        };

        if (fluid) {
            combinedStyle.maxWidth = "100%";
        } else if (typeof size === "number") {
            combinedStyle.maxWidth = size;
        } else if (typeof size === "string" && SIZE_MAP[size] !== undefined) {
            combinedStyle.maxWidth = SIZE_MAP[size];
        } else if (typeof size === "string") {
            combinedStyle.maxWidth = size;
        } else {
            combinedStyle.maxWidth = SIZE_MAP.md;
        }

        const cleaned = cleanLayoutProps(props);

        return <div ref={ref} className={className} style={combinedStyle} {...cleaned} />;
    },
);
Container.displayName = "Container";
