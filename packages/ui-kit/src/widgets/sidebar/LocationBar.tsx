import { Fragment, type ReactNode } from "react";
import styles from "./LocationBar.module.css";

export interface LocationSegment {
    id: string;
    label: ReactNode;
    /** Makes the segment a link back up the path. */
    onClick?: () => void;
}

export interface LocationBarProps {
    /** Path segments rendered as `a / b / c`; the last one is the current location. */
    segments: LocationSegment[];
    /** Muted text after the path (counts, FHIR version, …). */
    meta?: ReactNode;
    /** Right-aligned actions. */
    actions?: ReactNode;
    className?: string;
}

/**
 * Header that shows where you are as a REST-like path with Marigold slashes —
 * the same slash as the `o/` mark and the `Type/id` path chips.
 */
export function LocationBar({ segments, meta, actions, className }: LocationBarProps) {
    return (
        <header className={[styles.bar, className].filter(Boolean).join(" ")}>
            <nav className={styles.path} aria-label="Location">
                {segments.map((segment, index) => {
                    const last = index === segments.length - 1;
                    return (
                        <Fragment key={segment.id}>
                            {index > 0 && (
                                <span className={styles.slash} aria-hidden="true">
                                    /
                                </span>
                            )}
                            {segment.onClick && !last ? (
                                <button type="button" className={styles.segment} onClick={segment.onClick}>
                                    {segment.label}
                                </button>
                            ) : (
                                <span
                                    className={styles.segment}
                                    data-current={last ? "true" : undefined}
                                    aria-current={last ? "page" : undefined}
                                >
                                    {segment.label}
                                </span>
                            )}
                        </Fragment>
                    );
                })}
            </nav>
            {meta != null && <span className={styles.meta}>{meta}</span>}
            {actions != null && <div className={styles.actions}>{actions}</div>}
        </header>
    );
}
