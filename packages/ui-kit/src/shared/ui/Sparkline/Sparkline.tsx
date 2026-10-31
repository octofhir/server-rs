import classes from "./Sparkline.module.css";

export interface SparklineProps {
    /** Evenly spaced values; `null` breaks the line (a gap in the data). */
    values: (number | null)[];
    /** Height in px; the line fills the container's width. */
    height?: number;
    className?: string;
}

/**
 * A trend line without axes for stat cells. Scaled from 0 to the largest value so a flat
 * series reads as flat; stroked with the accent token, so it follows the theme.
 */
export function Sparkline({ values, height = 24, className }: SparklineProps) {
    const max = Math.max(0, ...values.map((value) => value ?? 0));
    const segments: string[] = [];
    if (values.length >= 2 && max > 0) {
        const step = 100 / (values.length - 1);
        let current = "";
        values.forEach((value, index) => {
            if (value == null) {
                if (current) segments.push(current);
                current = "";
                return;
            }
            const y = 23 - (value / max) * 21;
            current += `${current ? "L" : "M"}${(index * step).toFixed(2)},${y.toFixed(2)}`;
        });
        if (current) segments.push(current);
    }
    return (
        <svg
            className={[classes.root, className].filter(Boolean).join(" ")}
            style={{ height }}
            viewBox="0 0 100 24"
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {segments.length > 0 && <path d={segments.join(" ")} />}
        </svg>
    );
}
