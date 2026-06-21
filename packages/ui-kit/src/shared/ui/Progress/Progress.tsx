import {
    Progress as GravityProgress,
    type ProgressProps as GravityProgressProps,
    type ProgressTheme,
    type ProgressSize,
} from "@gravity-ui/uikit";

type ColorLike = ProgressTheme | "green" | "red" | "fire" | "yellow" | "amber" | "warm" | "blue" | "primary";

const THEME_BY_COLOR: Record<string, ProgressTheme> = {
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
    primary: "info",
    info: "info",
    misc: "misc",
    default: "default",
};

const SIZE_BY_NAME: Record<string, ProgressSize> = {
    xs: "xs",
    s: "s",
    sm: "s",
    m: "m",
    md: "m",
    lg: "m",
};

export type ProgressProps = Omit<GravityProgressProps, "theme" | "size"> & {
    /** Semantic color; resolved to a theme. */
    color?: ColorLike;
    theme?: ProgressTheme;
    size?: ProgressSize | "sm" | "md" | "lg";
};

/** Linear progress bar with a semantic `color` and flexible size names. */
export function Progress({ color, theme, size, ...props }: ProgressProps) {
    const resolvedTheme = theme ?? (color ? THEME_BY_COLOR[color] : undefined);
    const resolvedSize = size ? SIZE_BY_NAME[size] ?? "m" : undefined;
    return (
        <GravityProgress
            {...(props as GravityProgressProps)}
            theme={resolvedTheme}
            size={resolvedSize}
        />
    );
}
