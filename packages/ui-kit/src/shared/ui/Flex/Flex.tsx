import { forwardRef } from "react";
import { cleanLayoutProps, getSpacingStyles, mapSpaceValue, type SpacingProps } from "../layout-utils";

export interface FlexProps extends React.HTMLAttributes<HTMLDivElement>, SpacingProps {
    /** Gap between children. Accepts px numbers, CSS lengths, or `xs|sm|md|lg|xl` tokens. */
    gap?: number | string;
    align?: React.CSSProperties["alignItems"];
    justify?: React.CSSProperties["justifyContent"];
    direction?: React.CSSProperties["flexDirection"];
    wrap?: React.CSSProperties["flexWrap"] | boolean;
    /** Render as `inline-flex` instead of `flex`. */
    inline?: boolean;
}

export const Flex = forwardRef<HTMLDivElement, FlexProps>(
    ({ gap, align, justify, direction, wrap, inline, style, ...props }, ref) => {
        const combinedStyle: React.CSSProperties = {
            display: inline ? "inline-flex" : "flex",
            flexDirection: direction,
            alignItems: align,
            justifyContent: justify,
            flexWrap: typeof wrap === "boolean" ? (wrap ? "wrap" : "nowrap") : wrap,
            gap: gap !== undefined ? mapSpaceValue(gap) : undefined,
            ...getSpacingStyles(props),
            ...style,
        };
        const cleaned = cleanLayoutProps(props);
        return <div ref={ref} style={combinedStyle} {...cleaned} />;
    },
);
Flex.displayName = "Flex";

export const Stack = forwardRef<HTMLDivElement, FlexProps>((props, ref) => (
    <Flex ref={ref} direction="column" {...props} />
));
Stack.displayName = "Stack";

export const Group = forwardRef<HTMLDivElement, FlexProps>(({ wrap = "wrap", ...props }, ref) => (
    <Flex ref={ref} direction="row" wrap={wrap} {...props} />
));
Group.displayName = "Group";

export const Center = forwardRef<HTMLDivElement, FlexProps>((props, ref) => (
    <Flex ref={ref} align="center" justify="center" {...props} />
));
Center.displayName = "Center";
