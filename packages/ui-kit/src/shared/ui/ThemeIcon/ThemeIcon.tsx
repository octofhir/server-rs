import { forwardRef } from "react";

export interface ThemeIconProps extends React.HTMLAttributes<HTMLDivElement> {
    children?: React.ReactNode;
    size?: "xs" | "s" | "m" | "l" | "xl";
    view?: "normal" | "light" | "outlined";
    color?: "primary" | "positive" | "warning" | "danger" | "neutral";
}

export const ThemeIcon = forwardRef<HTMLDivElement, ThemeIconProps>(
    ({ children, size = "m", view = "light", color = "primary", style, ...props }, ref) => {
        const sizeMap = { xs: 16, s: 20, m: 24, l: 32, xl: 40 };
        const d = sizeMap[size];

        let bg = "transparent";
        let text = "var(--g-color-text-primary)";
        let border = "1px solid transparent";

        if (view === "light") {
            if (color === "primary") { bg = "var(--g-color-base-selection)"; text = "var(--g-color-text-brand)"; }
            else if (color === "positive") { bg = "var(--g-color-base-positive-hover)"; text = "var(--g-color-text-positive)"; }
            else if (color === "warning") { bg = "var(--g-color-base-warning-hover)"; text = "var(--g-color-text-warning)"; }
            else if (color === "danger") { bg = "var(--g-color-base-danger-hover)"; text = "var(--g-color-text-danger)"; }
            else { bg = "var(--g-color-base-generic)"; }
        } else if (view === "normal") {
            if (color === "primary") { bg = "var(--g-color-base-brand)"; text = "var(--g-color-text-light-primary)"; }
            else if (color === "positive") { bg = "var(--g-color-base-positive)"; text = "var(--g-color-text-light-primary)"; }
            else if (color === "warning") { bg = "var(--g-color-base-warning)"; text = "var(--g-color-text-light-primary)"; }
            else if (color === "danger") { bg = "var(--g-color-base-danger)"; text = "var(--g-color-text-light-primary)"; }
            else { bg = "var(--g-color-base-generic-hover)"; }
        } else if (view === "outlined") {
            if (color === "primary") { border = "1px solid var(--g-color-line-brand)"; text = "var(--g-color-text-brand)"; }
            else if (color === "positive") { border = "1px solid var(--g-color-line-positive)"; text = "var(--g-color-text-positive)"; }
            else if (color === "warning") { border = "1px solid var(--g-color-line-warning)"; text = "var(--g-color-text-warning)"; }
            else if (color === "danger") { border = "1px solid var(--g-color-line-danger)"; text = "var(--g-color-text-danger)"; }
            else { border = "1px solid var(--g-color-line-generic)"; }
        }

        return (
            <div
                ref={ref}
                style={{
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    width: d,
                    height: d,
                    borderRadius: 4,
                    backgroundColor: bg,
                    color: text,
                    border,
                    ...style,
                }}
                {...props}
            >
                {children}
            </div>
        );
    }
);
ThemeIcon.displayName = "ThemeIcon";
