import type { ReactNode } from "react";
import styles from "./Shell.module.css";

export interface ShellProps {
    /** The sidebar element (typically `<Sidebar />`). */
    sidebar: ReactNode;
    /** Optional top bar above the content (typically `<LocationBar />`). */
    header?: ReactNode;
    children?: ReactNode;
    className?: string;
}

/** App layout: fixed sidebar on the left, optional header, scrollable main content area. */
export function Shell({ sidebar, header, children, className }: ShellProps) {
    return (
        <div className={[styles.shell, className].filter(Boolean).join(" ")}>
            {sidebar}
            <div className={styles.column}>
                {header}
                <main className={styles.main}>{children}</main>
            </div>
        </div>
    );
}
