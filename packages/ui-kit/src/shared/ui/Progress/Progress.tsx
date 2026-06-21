import styles from "./Progress.module.css";

type Tone = "primary" | "success" | "danger" | "warning" | "info";

const TONE_BY_COLOR: Record<string, Tone> = {
    green: "success",
    success: "success",
    red: "danger",
    fire: "danger",
    danger: "danger",
    yellow: "warning",
    amber: "warning",
    warm: "warning",
    warning: "warning",
    blue: "info",
    info: "info",
    primary: "primary",
    default: "primary",
};

const SIZE_BY_NAME: Record<string, "xs" | "s" | "m"> = {
    xs: "xs",
    s: "s",
    sm: "s",
    m: "m",
    md: "m",
    lg: "m",
};

export interface ProgressProps {
    /** Completion percentage (0–100). */
    value?: number;
    /** Semantic color. */
    color?: string;
    /** Alias resolved to a color tone. */
    theme?: string;
    size?: "xs" | "s" | "m" | "sm" | "md" | "lg";
    striped?: boolean;
    animated?: boolean;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

/** Linear progress bar driven by `--octo-*` tokens. */
export function Progress({
    value = 0,
    color,
    theme,
    size = "m",
    striped,
    animated,
    className,
    style,
    "aria-label": ariaLabel,
}: ProgressProps) {
    const tone = TONE_BY_COLOR[color ?? theme ?? "primary"] ?? "primary";
    const resolvedSize = SIZE_BY_NAME[size] ?? "m";
    const pct = Math.max(0, Math.min(100, value));
    return (
        <div
            className={[styles.track, className].filter(Boolean).join(" ")}
            data-size={resolvedSize}
            style={style}
            role="progressbar"
            aria-valuenow={pct}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={ariaLabel}
        >
            <div
                className={styles.bar}
                data-tone={tone}
                data-striped={striped || undefined}
                data-animated={animated || undefined}
                style={{ width: `${pct}%` }}
            />
        </div>
    );
}
