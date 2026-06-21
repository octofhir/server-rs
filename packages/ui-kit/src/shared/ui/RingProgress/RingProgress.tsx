import { forwardRef, type ReactNode } from "react";

export interface RingProgressSection {
    /** Portion of the ring, 0–100. */
    value: number;
    /** Stroke color (CSS color or token var). */
    color: string;
    tooltip?: string;
}

export interface RingProgressProps {
    /** Outer diameter in px. */
    size?: number;
    /** Ring stroke width in px. */
    thickness?: number;
    /** Ordered segments; values are percentages of the full circle. */
    sections: RingProgressSection[];
    /** Round the segment ends. */
    roundCaps?: boolean;
    /** Centered content (label / value). */
    label?: ReactNode;
    className?: string;
}

/** Determinate multi-segment ring chart. */
export const RingProgress = forwardRef<HTMLDivElement, RingProgressProps>(function RingProgress(
    { size = 120, thickness = 12, sections, roundCaps, label, className },
    ref,
) {
    const radius = (size - thickness) / 2;
    const circumference = 2 * Math.PI * radius;
    const center = size / 2;

    let offset = 0;
    const arcs = sections.map((section, index) => {
        const clamped = Math.max(0, Math.min(100, section.value));
        const length = (clamped / 100) * circumference;
        const dasharray = `${length} ${circumference - length}`;
        const dashoffset = -offset;
        offset += length;
        return (
            <circle
                key={`seg-${index}-${section.color}`}
                cx={center}
                cy={center}
                r={radius}
                fill="none"
                stroke={section.color}
                strokeWidth={thickness}
                strokeDasharray={dasharray}
                strokeDashoffset={dashoffset}
                strokeLinecap={roundCaps ? "round" : "butt"}
            />
        );
    });

    return (
        <div
            ref={ref}
            className={className}
            style={{ position: "relative", width: size, height: size, display: "inline-flex" }}
        >
            <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }}>
                <circle
                    cx={center}
                    cy={center}
                    r={radius}
                    fill="none"
                    stroke="var(--g-color-line-generic)"
                    strokeWidth={thickness}
                />
                {arcs}
            </svg>
            {label != null && (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        textAlign: "center",
                    }}
                >
                    {label}
                </div>
            )}
        </div>
    );
});
