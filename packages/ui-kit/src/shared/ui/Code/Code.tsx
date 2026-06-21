import { forwardRef, type ForwardedRef, type HTMLAttributes } from "react";
import { cleanLayoutProps, getSpacingStyles, type SpacingProps } from "../layout-utils";

export interface CodeProps extends HTMLAttributes<HTMLElement>, SpacingProps {
    block?: boolean;
    color?: string;
}

function assignRef(ref: ForwardedRef<HTMLElement>, value: HTMLElement | null): void {
    if (typeof ref === "function") {
        ref(value);
        return;
    }
    if (ref) {
        ref.current = value;
    }
}

export const Code = forwardRef<HTMLElement, CodeProps>(({ block, color, style, className, ...props }, ref) => {
    const merged: React.CSSProperties = {
        fontFamily: "var(--g-font-family-monospace, var(--octo-typography-mono))",
        fontSize: "0.875em",
        background: "var(--octo-surface-2, #f1f3f5)",
        color: "var(--octo-text-primary, #1a1b1e)",
        padding: block ? "12px 14px" : "1px 6px",
        borderRadius: 6,
        display: block ? "block" : "inline",
        whiteSpace: block ? "pre" : "normal",
        ...getSpacingStyles(props),
        ...(color ? { color } : {}),
        ...style,
    };
    const cleaned = cleanLayoutProps(props);
    const setRef = (node: HTMLElement | null) => assignRef(ref, node);
    return block ? (
        <pre ref={setRef} className={className} style={merged} {...cleaned} />
    ) : (
        <code ref={setRef} className={className} style={merged} {...cleaned} />
    );
});
Code.displayName = "Code";
