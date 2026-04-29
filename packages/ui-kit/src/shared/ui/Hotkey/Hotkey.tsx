import { forwardRef } from "react";
import { Hotkey as GravityHotkey, type HotkeyProps as GravityHotkeyProps } from "@gravity-ui/uikit";

export type HotkeyProps = GravityHotkeyProps;

export const Hotkey = forwardRef<HTMLElement, HotkeyProps>(function Hotkey(props, ref) {
    return <GravityHotkey ref={ref} {...props} />;
});
