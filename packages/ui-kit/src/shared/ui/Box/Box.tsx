import React, { forwardRef } from "react";
import { Box as GravityBox, type BoxProps as GravityBoxProps } from "@gravity-ui/uikit";
import { getSpacingStyles, cleanLayoutProps } from "../layout-utils";

export interface BoxProps extends GravityBoxProps {
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

export const Box = forwardRef<HTMLDivElement, BoxProps>((props, ref) => {
    const style = { ...getSpacingStyles(props), ...props.style };
    const cleaned = cleanLayoutProps(props);
    return <GravityBox ref={ref} {...cleaned} style={style} />;
});
Box.displayName = "Box";
