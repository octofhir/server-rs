import {
    Group,
    Panel,
    Separator,
    type GroupProps,
    type PanelProps,
    type SeparatorProps,
} from "react-resizable-panels";
import classes from "./ResizablePanels.module.css";

/**
 * Standard Resizable Panel Group.
 * Supports horizontal or vertical layouts via the `orientation` prop.
 */
export function ResizableGroup({ groupRef, ...props }: GroupProps) {
    return <Group groupRef={groupRef} {...props} />;
}

/**
 * Individual Resizable Pane.
 * Supports collapse, sizing limits, and panelRef-based imperative control.
 */
export function ResizablePane({ panelRef, ...props }: PanelProps) {
    return <Panel panelRef={panelRef} {...props} />;
}

export interface ResizableHandleProps extends SeparatorProps {
    className?: string;
}

/**
 * Styled Resize Handle.
 * Renders a thin, sleek separator line that lights up with a primary blue/violet accent glow on hover or active dragging.
 */
export function ResizableHandle({ className, ...props }: ResizableHandleProps) {
    return (
        <Separator
            className={[classes.handle, className].filter(Boolean).join(" ")}
            {...props}
        >
            <div className={classes.line} />
        </Separator>
    );
}

export const Resizable = {
    Group: ResizableGroup,
    Pane: ResizablePane,
    Handle: ResizableHandle,
};
export type { GroupImperativeHandle as ResizableGroupHandle, PanelImperativeHandle as ResizablePaneHandle } from "react-resizable-panels";
