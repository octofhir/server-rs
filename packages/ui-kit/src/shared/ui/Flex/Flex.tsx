import React, { forwardRef } from "react";
import { Flex as GravityFlex, type FlexProps as GravityFlexProps } from "@gravity-ui/uikit";
import { getSpacingStyles, cleanLayoutProps, mapSpaceValue } from "../layout-utils";

type GravityGap = Extract<NonNullable<GravityFlexProps["gap"]>, number | string>;

const GRAVITY_GAPS = [0, 0.5, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as const satisfies readonly GravityGap[];

function toGravityGap(gap: number | string | undefined): GravityGap | undefined {
    const gapPx = mapSpaceValue(gap);
    if (typeof gapPx !== "number") {
        return undefined;
    }

    const gravityGap = Math.round(gapPx / 4);
    return GRAVITY_GAPS.find((value) => value === gravityGap);
}

export interface FlexProps extends Omit<GravityFlexProps, "gap" | "direction" | "wrap"> {
    gap?: number | string;
    align?: GravityFlexProps["alignItems"];
    justify?: GravityFlexProps["justifyContent"];
    direction?: GravityFlexProps["direction"];
    wrap?: GravityFlexProps["wrap"];
    style?: React.CSSProperties;
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

export const Flex = forwardRef<HTMLDivElement, FlexProps>(({ gap, align, justify, direction, wrap, style, ...props }, ref) => {
    const combinedStyle = { ...getSpacingStyles(props), ...style };
    const cleaned = cleanLayoutProps(props);
    const gravityGap = toGravityGap(gap);

    return (
        <GravityFlex 
            ref={ref} 
            gap={gravityGap}
            alignItems={align}
            justifyContent={justify}
            direction={direction}
            wrap={wrap}
            style={combinedStyle} 
            {...cleaned} 
        />
    );
});
Flex.displayName = "Flex";

export const Stack = forwardRef<HTMLDivElement, FlexProps>((props, ref) => <Flex ref={ref} direction="column" {...props} />);
Stack.displayName = "Stack";

export const Group = forwardRef<HTMLDivElement, FlexProps>(({ wrap = "wrap", ...props }, ref) => <Flex ref={ref} direction="row" wrap={wrap} {...props} />);
Group.displayName = "Group";

export const Center = forwardRef<HTMLDivElement, FlexProps>((props, ref) => <Flex ref={ref} align="center" justify="center" {...props} />);
Center.displayName = "Center";
