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
    /** Open delay in ms. */
    delay?: number;
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
export function Tooltip({ label, placement = "top", delay = 300, children }: TooltipProps) {
    if (label == null || label === "") {
        return <>{children}</>;
    }
    const { side, align } = parsePlacement(placement);
    const trigger = isValidElement(children) ? (children as ReactElement) : <span>{children}</span>;

    return (
        <BaseTooltip.Provider delay={delay}>
            <BaseTooltip.Root>
                <BaseTooltip.Trigger render={trigger} />
                <BaseTooltip.Portal>
                    <BaseTooltip.Positioner side={side} align={align} sideOffset={6}>
                        <BaseTooltip.Popup className={styles.popup}>{label}</BaseTooltip.Popup>
                    </BaseTooltip.Positioner>
                </BaseTooltip.Portal>
            </BaseTooltip.Root>
        </BaseTooltip.Provider>
    );
}
