import { forwardRef, type HTMLAttributes, type ReactNode } from "react";
import classes from "./PageLayout.module.css";

export interface PageContainerProps extends HTMLAttributes<HTMLDivElement> {
    children: ReactNode;
}

/**
 * PageContainer provides a fixed layout boundary for a page.
 * Fits exactly the viewport height bounds, preventing double scrollbars.
 */
export const PageContainer = forwardRef<HTMLDivElement, PageContainerProps>(
    function PageContainer({ children, className, ...props }, ref) {
        return (
            <div
                ref={ref}
                className={[classes.container, className].filter(Boolean).join(" ")}
                {...props}
            >
                {children}
            </div>
        );
    }
);

export interface ScrollableContentProps extends HTMLAttributes<HTMLDivElement> {
    children: ReactNode;
}

/**
 * ScrollableContent provides an independent scrolling container for page contents.
 * Keeps the page header fixed while allowing body elements to scroll natively.
 */
export const ScrollableContent = forwardRef<HTMLDivElement, ScrollableContentProps>(
    function ScrollableContent({ children, className, ...props }, ref) {
        return (
            <div
                ref={ref}
                className={[classes.scrollable, className].filter(Boolean).join(" ")}
                {...props}
            >
                {children}
            </div>
        );
    }
);
