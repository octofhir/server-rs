import React, { forwardRef } from "react";
import {
    Button as GravityButton,
    type ButtonPin,
    type ButtonSize,
    type ButtonView,
    type ButtonWidth,
} from "@gravity-ui/uikit";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

type LegacyButtonVariant = "default" | "filled" | "light" | "outline" | "subtle" | "transparent";
type LegacyButtonColor =
    | "primary"
    | "blue"
    | "red"
    | "fire"
    | "green"
    | "orange"
    | "yellow"
    | "warm"
    | "gray"
    | "deep"
    | string;

export interface ButtonProps
    extends Omit<
        React.ButtonHTMLAttributes<HTMLButtonElement> &
            React.AnchorHTMLAttributes<HTMLAnchorElement>,
        "color" | "style"
    > {
    view?: ButtonView;
    size?: ButtonSize;
    pin?: ButtonPin;
    selected?: boolean;
    disabled?: boolean;
    loading?: boolean;
    width?: ButtonWidth;
    href?: string;
    component?: React.ElementType;
    className?: string;
    style?: React.CSSProperties;
    children?: React.ReactNode;
    qa?: string;

    /** @deprecated Mantine compatibility. Use `view` and `Button.Icon`. */
    variant?: LegacyButtonVariant;
    /** @deprecated Mantine compatibility. Encoded into Gravity `view`. */
    color?: LegacyButtonColor;
    /** @deprecated Mantine compatibility. Use `Button.Icon`. */
    leftSection?: React.ReactNode;
    /** @deprecated Mantine compatibility. Use `Button.Icon`. */
    rightSection?: React.ReactNode;
    /** @deprecated Mantine compatibility. Gravity buttons use kit-defined pins. */
    radius?: string | number;
    /** @deprecated Mantine compatibility. Use `width="max"`. */
    fullWidth?: boolean;
    /** Layout compatibility props used by legacy screens. */
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

const COLOR_SUFFIX: Record<string, "action" | "info" | "success" | "warning" | "danger" | "utility"> = {
    primary: "action",
    blue: "info",
    deep: "info",
    indigo: "info",
    green: "success",
    red: "danger",
    fire: "danger",
    orange: "warning",
    yellow: "warning",
    warm: "warning",
    gray: "utility",
};

function colorSuffix(color?: LegacyButtonColor) {
    return color ? COLOR_SUFFIX[color] : undefined;
}

function mapView(variant?: LegacyButtonVariant, color?: LegacyButtonColor): ButtonView {
    const suffix = colorSuffix(color);

    if (variant === "subtle" || variant === "transparent") {
        return suffix ? `flat-${suffix}` as ButtonView : "flat";
    }
    if (variant === "light" || variant === "outline") {
        return suffix ? `outlined-${suffix}` as ButtonView : "outlined";
    }
    if (variant === "filled") {
        if (suffix === "danger") return "outlined-danger";
        if (suffix === "success") return "outlined-success";
        if (suffix === "warning") return "outlined-warning";
        return "action";
    }

    return color ? mapView("light", color) : "normal";
}

const ButtonRoot = forwardRef<HTMLButtonElement, ButtonProps>(
    (
        {
            view,
            variant,
            color,
            leftSection,
            rightSection,
            radius: _radius,
            fullWidth,
            width,
            style,
            children,
            ...props
        },
        ref,
    ) => {
        const ButtonComponent: React.ElementType = GravityButton;
        const cleaned = cleanLayoutProps(props);
        const mergedStyle = { ...getSpacingStyles(props), ...style };

        return (
            <ButtonComponent
                ref={ref}
                view={view ?? mapView(variant, color)}
                width={fullWidth ? "max" : width}
                style={mergedStyle}
                {...cleaned}
            >
                {leftSection ? <GravityButton.Icon side="left">{leftSection}</GravityButton.Icon> : null}
                {children}
                {rightSection ? <GravityButton.Icon side="right">{rightSection}</GravityButton.Icon> : null}
            </ButtonComponent>
        );
    },
);

ButtonRoot.displayName = "Button";

export const Button = Object.assign(ButtonRoot, {
    Icon: GravityButton.Icon,
});
