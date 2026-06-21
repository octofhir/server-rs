import type { ReactNode } from "react";
import styles from "./Shell.module.css";

export interface ShellProps {
    /** The sidebar element (typically `<Sidebar />`). */
    sidebar: ReactNode;
    children?: ReactNode;
    className?: string;
}

/** App layout: fixed sidebar on the left, scrollable main content area. */
export function Shell({ sidebar, children, className }: ShellProps) {
    return (
        <div className={[styles.shell, className].filter(Boolean).join(" ")}>
            {sidebar}
            <main className={styles.main}>{children}</main>
        </div>
    );
}
