import React, { forwardRef } from "react";
import { Button as BaseButton } from "@base-ui/react/button";
import { cleanLayoutProps, getSpacingStyles, type Size } from "../layout-utils";
import styles from "./Button.module.css";

export type ButtonView =
    | "normal"
    | "action"
    | "outlined"
    | "flat"
    | "raised"
    | "flat-secondary"
    | `${"action" | "outlined" | "flat" | "raised" | "normal"}-${"info" | "success" | "warning" | "danger" | "utility"}`;

export type ButtonSize = Size;
export type ButtonPin = "round-round" | "circle" | "brick" | string;
export type ButtonWidth = "auto" | "max" | "fit";

type ButtonVariant = "default" | "filled" | "light" | "outline" | "subtle" | "transparent";
type ButtonColor =
    | "primary" | "blue" | "red" | "fire" | "green" | "orange" | "yellow" | "warm" | "gray" | "deep" | string;

export interface ButtonProps
    extends Omit<
        React.ButtonHTMLAttributes<HTMLButtonElement> & React.AnchorHTMLAttributes<HTMLAnchorElement>,
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

    /** Shorthand visual style; combined with `color` to resolve a `view`. */
    variant?: ButtonVariant;
    /** Shorthand semantic color; combined with `variant` to resolve a `view`. */
    color?: ButtonColor;
    /** Icon rendered before the label. */
    leftSection?: React.ReactNode;
    /** Icon rendered after the label. */
    rightSection?: React.ReactNode;
    radius?: string | number;
    /** Stretch to the full width of the container. */
    fullWidth?: boolean;
    w?: number | string; h?: number | string;
    p?: number | string; px?: number | string; py?: number | string;
    pt?: number | string; pb?: number | string; pl?: number | string; pr?: number | string;
    m?: number | string; mx?: number | string; my?: number | string;
    mt?: number | string; mb?: number | string; ml?: number | string; mr?: number | string;
}

const TONES = new Set(["info", "success", "warning", "danger", "utility"]);

const COLOR_TONE: Record<string, "info" | "success" | "warning" | "danger" | "utility"> = {
    primary: "info", blue: "info", deep: "info", indigo: "info",
    green: "success", red: "danger", fire: "danger",
    orange: "warning", yellow: "warning", warm: "warning", gray: "utility",
};

function resolveView(variant?: ButtonVariant, color?: ButtonColor): ButtonView {
    const tone = color ? COLOR_TONE[color] : undefined;
    let base: string;
    if (variant === "subtle" || variant === "transparent") base = "flat";
    else if (variant === "light" || variant === "outline") base = "outlined";
    else if (variant === "filled") base = "action";
    else base = color ? "outlined" : "normal";
    return (tone ? `${base}-${tone}` : base) as ButtonView;
}

function parseView(view: string): { base: string; tone?: string } {
    const [base, maybeTone] = view.split("-");
    if (maybeTone && TONES.has(maybeTone)) return { base, tone: maybeTone };
    return { base };
}

interface ButtonIconProps {
    children?: React.ReactNode;
    side?: "left" | "right";
    className?: string;
}

function ButtonIcon({ children, className }: ButtonIconProps) {
    return <span className={[styles.icon, className].filter(Boolean).join(" ")}>{children}</span>;
}

const ButtonRoot = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
    {
        view,
        variant,
        color,
        size = "md",
        pin = "round-round",
        selected,
        disabled,
        loading,
        width,
        fullWidth,
        href,
        component,
        leftSection,
        rightSection,
        radius: _radius,
        className,
        style,
        children,
        qa,
        ...rest
    },
    ref,
) {
    const resolvedView = view ?? resolveView(variant, color);
    const { base, tone } = parseView(resolvedView);
    const mergedStyle = { ...getSpacingStyles(rest), ...style };
    const cleaned = cleanLayoutProps(rest);

    const dataProps = {
        "data-base": base,
        "data-tone": tone,
        "data-size": size,
        "data-pin": pin === "round-round" ? undefined : pin,
        "data-width": fullWidth ? "max" : width === "max" ? "max" : undefined,
        "data-selected": selected ? "true" : undefined,
        "data-loading": loading ? "true" : undefined,
        "data-qa": qa,
    };

    const content = (
        <>
            {leftSection != null && <ButtonIcon side="left">{leftSection}</ButtonIcon>}
            {children}
            {rightSection != null && <ButtonIcon side="right">{rightSection}</ButtonIcon>}
            {loading && <span className={styles.spinner} aria-hidden="true" />}
        </>
    );

    const cls = [styles.button, className].filter(Boolean).join(" ");

    const renderAs = component ?? (href != null ? "a" : undefined);
    if (renderAs) {
        return (
            <BaseButton
                ref={ref as React.Ref<HTMLElement>}
                render={(renderProps) =>
                    React.createElement(
                        renderAs,
                        renderAs === "a" && !component ? { ...renderProps, href } : renderProps,
                    )
                }
                className={cls}
                disabled={disabled || loading}
                style={mergedStyle}
                {...dataProps}
                {...cleaned}
            >
                {content}
            </BaseButton>
        );
    }

    return (
        <BaseButton
            ref={ref as React.Ref<HTMLElement>}
            className={cls}
            disabled={disabled || loading}
            style={mergedStyle}
            {...dataProps}
            {...cleaned}
        >
            {content}
        </BaseButton>
    );
});

export const Button = Object.assign(ButtonRoot, {
    Icon: ButtonIcon,
});
