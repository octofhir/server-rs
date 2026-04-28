import { forwardRef, type HTMLAttributes } from "react";

export interface CodeProps extends HTMLAttributes<HTMLElement> {
    block?: boolean;
}

export const Code = forwardRef<HTMLElement, CodeProps>(({ block, style, className, ...props }, ref) => {
    const Tag = block ? "pre" : "code";
    const merged: React.CSSProperties = {
        fontFamily: "var(--g-font-family-monospace, var(--octo-typography-mono))",
        fontSize: "0.875em",
        background: "var(--g-color-base-misc-light, var(--g-color-base-generic))",
        color: "var(--g-color-text-primary)",
        padding: block ? "12px 14px" : "1px 6px",
        borderRadius: 6,
        display: block ? "block" : "inline",
        whiteSpace: block ? "pre" : "normal",
        ...style,
    };
    return <Tag ref={ref as never} className={className} style={merged} {...props} />;
});
Code.displayName = "Code";
