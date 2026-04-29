import { forwardRef } from "react";
import { Label, type LabelProps } from "@gravity-ui/uikit";

export type BadgeProps = LabelProps;

export const Badge = forwardRef<HTMLDivElement, BadgeProps>(function Badge(props, ref) {
    return <Label ref={ref} {...props} />;
});
