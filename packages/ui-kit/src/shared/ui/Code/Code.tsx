import { forwardRef, type ForwardedRef, type HTMLAttributes } from "react";

export interface CodeProps extends HTMLAttributes<HTMLElement> {
    block?: boolean;
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

export const Code = forwardRef<HTMLElement, CodeProps>(({ block, style, className, ...props }, ref) => {
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
    const setRef = (node: HTMLElement | null) => assignRef(ref, node);
    return block ? (
        <pre ref={setRef} className={className} style={merged} {...props} />
    ) : (
        <code ref={setRef} className={className} style={merged} {...props} />
    );
});
Code.displayName = "Code";
