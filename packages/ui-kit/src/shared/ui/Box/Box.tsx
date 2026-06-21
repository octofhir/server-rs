import { forwardRef } from "react";
import { cleanLayoutProps, getSpacingStyles, type SpacingProps } from "../layout-utils";

export interface BoxProps extends React.HTMLAttributes<HTMLDivElement>, SpacingProps {}

export const Box = forwardRef<HTMLDivElement, BoxProps>((props, ref) => {
    const style = { ...getSpacingStyles(props), ...props.style };
    const cleaned = cleanLayoutProps(props);
    return <div ref={ref} {...cleaned} style={style} />;
});
Box.displayName = "Box";
