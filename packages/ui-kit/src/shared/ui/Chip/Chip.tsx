import { forwardRef, useCallback, type MouseEvent } from "react";
import { Label, type LabelProps } from "@gravity-ui/uikit";

export interface ChipProps extends Omit<LabelProps, "interactive" | "onClick"> {
    /** Selected state. */
    checked?: boolean;
    /** Toggle callback. Receives the new checked value. */
    onChange?: (next: boolean) => void;
}

export const Chip = forwardRef<HTMLDivElement, ChipProps>(function Chip(
    { checked = false, onChange, theme, ...rest },
    ref,
) {
    const resolvedTheme: LabelProps["theme"] = checked ? theme ?? "info" : "unknown";
    const handleClick = useCallback(
        (_e: MouseEvent<HTMLDivElement>) => {
            onChange?.(!checked);
        },
        [checked, onChange],
    );

    return (
        <Label
            ref={ref}
            theme={resolvedTheme}
            interactive={Boolean(onChange)}
            onClick={onChange ? handleClick : undefined}
            {...rest}
        />
    );
});
