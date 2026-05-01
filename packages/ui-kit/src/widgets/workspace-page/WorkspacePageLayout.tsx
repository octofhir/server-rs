import type { CSSProperties, ReactNode } from "react";
import { Text } from "#/shared/ui";
import classes from "./WorkspacePageLayout.module.css";

export interface WorkspacePageLayoutProps {
    title: ReactNode;
    description?: ReactNode;
    kicker?: ReactNode;
    meta?: ReactNode;
    actions?: ReactNode;
    toolbar?: ReactNode;
    aside?: ReactNode;
    children: ReactNode;
    maxWidth?: number | string;
    className?: string;
    bodyClassName?: string;
    contentClassName?: string;
}

export interface WorkspacePageSectionProps {
    title?: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    children: ReactNode;
    className?: string;
    contentClassName?: string;
}

function joinClassNames(...values: Array<string | undefined | false>) {
    return values.filter(Boolean).join(" ");
}

function maxWidthStyle(maxWidth?: number | string): CSSProperties | undefined {
    if (maxWidth === undefined) return undefined;
    return { "--workspace-page-max-width": typeof maxWidth === "number" ? `${maxWidth}px` : maxWidth } as CSSProperties;
}

export function WorkspacePageLayout({
    title,
    description,
    kicker,
    meta,
    actions,
    toolbar,
    aside,
    children,
    maxWidth,
    className,
    bodyClassName,
    contentClassName,
}: WorkspacePageLayoutProps) {
    return (
        <div className={joinClassNames(classes.root, className)} style={maxWidthStyle(maxWidth)}>
            <header className={classes.header}>
                <div className={classes.headerInner}>
                    <div className={classes.headingRow}>
                        <div className={classes.titleBlock}>
                            {kicker ? <div className={classes.kickerRow}>{kicker}</div> : null}
                            <Text as="h1" variant="display-1" className={classes.title}>
                                {title}
                            </Text>
                            {description ? (
                                <Text
                                    as="div"
                                    variant="body-2"
                                    color="secondary"
                                    className={classes.description}
                                >
                                    {description}
                                </Text>
                            ) : null}
                        </div>

                        {actions ? <div className={classes.actions}>{actions}</div> : null}
                    </div>

                    {meta ? <div className={classes.meta}>{meta}</div> : null}
                    {toolbar ? <div className={classes.toolbar}>{toolbar}</div> : null}
                </div>
            </header>

            <div className={joinClassNames(classes.body, bodyClassName)}>
                <div
                    className={joinClassNames(
                        classes.bodyInner,
                        aside ? classes.bodyWithAside : undefined,
                        contentClassName,
                    )}
                >
                    <main className={classes.main}>{children}</main>
                    {aside ? <aside className={classes.aside}>{aside}</aside> : null}
                </div>
            </div>
        </div>
    );
}

export function WorkspacePageSection({
    title,
    description,
    actions,
    children,
    className,
    contentClassName,
}: WorkspacePageSectionProps) {
    const hasHeader = title || description || actions;

    return (
        <section className={joinClassNames(classes.section, className)}>
            {hasHeader ? (
                <div className={classes.sectionHeader}>
                    <div className={classes.sectionTitleBlock}>
                        {title ? (
                            <Text as="h2" variant="header-2">
                                {title}
                            </Text>
                        ) : null}
                        {description ? (
                            <Text as="div" variant="body-1" color="secondary">
                                {description}
                            </Text>
                        ) : null}
                    </div>
                    {actions ? <div className={classes.sectionActions}>{actions}</div> : null}
                </div>
            ) : null}

            <div className={joinClassNames(classes.sectionContent, contentClassName)}>
                {children}
            </div>
        </section>
    );
}
