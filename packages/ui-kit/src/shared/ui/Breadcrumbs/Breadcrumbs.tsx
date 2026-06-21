import { Children, forwardRef, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import styles from "./Breadcrumbs.module.css";

export interface BreadcrumbsProps {
    /** Each child is one crumb; separators are inserted automatically. */
    children?: ReactNode;
    /** Custom separator between crumbs. */
    separator?: ReactNode;
    className?: string;
}

export const Breadcrumbs = forwardRef<HTMLElement, BreadcrumbsProps>(function Breadcrumbs(
    { children, separator, className },
    ref,
) {
    const items = Children.toArray(children).filter(Boolean);
    return (
        <nav
            ref={ref}
            aria-label="Breadcrumb"
            className={[styles.nav, className].filter(Boolean).join(" ")}
        >
            <ol className={styles.list}>
                {items.map((item, i) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: crumb order is stable
                    <li key={i} className={styles.item}>
                        {item}
                        {i < items.length - 1 && (
                            <span className={styles.separator} aria-hidden="true">
                                {separator ?? <ChevronRight size={14} />}
                            </span>
                        )}
                    </li>
                ))}
            </ol>
        </nav>
    );
});
