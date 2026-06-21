import { isValidElement, type ReactElement, type ReactNode } from "react";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import styles from "./Tooltip.module.css";

type Side = "top" | "right" | "bottom" | "left";
type Align = "start" | "center" | "end";
export type TooltipPlacement =
    | Side
    | `${Side}-start`
    | `${Side}-end`;

export interface TooltipProps {
    /** Tooltip text. When empty, the trigger renders without a tooltip. */
    label?: ReactNode;
    placement?: TooltipPlacement;
    /** Alias of {@link placement}. */
    position?: TooltipPlacement;
    /** Open delay in ms. */
    delay?: number;
    /** Alias of {@link delay}. */
    openDelay?: number;
    /** Max width (px or CSS length) for multi-line tooltips. */
    w?: number | string;
    /** Accepted for API parity; the popup already wraps long text. */
    multiline?: boolean;
    /** Accepted for API parity; the kit tooltip renders without an arrow. */
    withArrow?: boolean;
    /** The trigger element. */
    children: ReactNode;
}

function parsePlacement(placement: TooltipPlacement): { side: Side; align: Align } {
    const [side, sub] = placement.split("-") as [Side, "start" | "end" | undefined];
    return { side, align: sub ?? "center" };
}

/**
 * Hover/focus tooltip (Base UI). Pass the trigger as `children` and the text as
 * `label`. Self-contained Provider so it can be dropped anywhere.
 */
export function Tooltip({ label, placement, position, delay, openDelay, w, children }: TooltipProps) {
    if (label == null || label === "") {
        return <>{children}</>;
    }
    const { side, align } = parsePlacement(placement ?? position ?? "top");
    const trigger = isValidElement(children) ? (children as ReactElement) : <span>{children}</span>;

    return (
        <BaseTooltip.Provider delay={delay ?? openDelay ?? 300}>
            <BaseTooltip.Root>
                <BaseTooltip.Trigger render={trigger} />
                <BaseTooltip.Portal>
                    <BaseTooltip.Positioner side={side} align={align} sideOffset={6}>
                        <BaseTooltip.Popup className={styles.popup} style={w != null ? { maxWidth: w } : undefined}>
                            {label}
                        </BaseTooltip.Popup>
                    </BaseTooltip.Positioner>
                </BaseTooltip.Portal>
            </BaseTooltip.Root>
        </BaseTooltip.Provider>
    );
}
