import { forwardRef, type ReactNode } from "react";
import type { Size } from "../layout-utils";
import styles from "./Hotkey.module.css";

export interface HotkeyProps {
    /** Key combination, e.g. `mod+k` or `Shift+Enter`. */
    value?: string;
    children?: ReactNode;
    className?: string;
    size?: Size;
}

const SYMBOLS: Record<string, string> = {
    mod: "⌘",
    cmd: "⌘",
    command: "⌘",
    meta: "⌘",
    ctrl: "Ctrl",
    control: "Ctrl",
    shift: "⇧",
    alt: "⌥",
    option: "⌥",
    enter: "↵",
    return: "↵",
    escape: "Esc",
    esc: "Esc",
    backspace: "⌫",
    delete: "Del",
    tab: "⇥",
    space: "Space",
    up: "↑",
    down: "↓",
    left: "←",
    right: "→",
};

function prettify(key: string): string {
    const symbol = SYMBOLS[key.toLowerCase()];
    if (symbol) return symbol;
    return key.length === 1 ? key.toUpperCase() : key;
}

export const Hotkey = forwardRef<HTMLSpanElement, HotkeyProps>(function Hotkey(
    { value, children, className, size },
    ref,
) {
    const raw = value ?? (typeof children === "string" ? children : "");
    const keys = raw.split(/[+\s]+/).filter(Boolean);
    return (
        <span
            ref={ref}
            data-size={size}
            className={[styles.hotkey, className].filter(Boolean).join(" ")}
        >
            {keys.map((key, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: keys are positional and static
                <kbd key={i} className={styles.key}>
                    {prettify(key)}
                </kbd>
            ))}
        </span>
    );
});
