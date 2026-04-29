import React, { forwardRef } from "react";
import { Hotkey as GravityHotkey, type HotkeyProps as GravityHotkeyProps } from "@gravity-ui/uikit";

export interface HotkeyProps extends Omit<GravityHotkeyProps, "value"> {
    value?: string;
    children?: React.ReactNode;
}

export const Hotkey = forwardRef<HTMLElement, HotkeyProps>(function Hotkey({ value, children, ...props }, ref) {
    // If value is not provided, try to use children as the value (Mantine compatibility)
    const hotkeyValue = value || (typeof children === "string" ? children : undefined);
    
    // Gravity Hotkey requires a string value. If we don't have one, we can't render it correctly as a Hotkey.
    // However, if we don't have a value, we should probably not call split() on undefined in the internal library.
    // By passing an empty string or ensuring it's not undefined, we avoid the crash.
    return <GravityHotkey ref={ref} value={hotkeyValue || ""} {...props} />;
});
