import { createContext, forwardRef, useContext, useMemo, type CSSProperties, type HTMLAttributes, type ReactNode } from "react";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

/**
 * Product application shell.
 *
 * Layout (CSS grid):
 *   ┌────────────────────────────────┐
 *   │ Header (full width, fixed h)   │
 *   ├──────────┬─────────────────────┤
 *   │  Navbar  │      Main           │
 *   │ (sticky) │   (scroll content)  │
 *   └──────────┴─────────────────────┘
 *
 * Mirrors the API from `@gravity-ui/navigation`'s `AsideHeader` enough to
 * support a sidebar that collapses on mobile and a sticky header.
 *
 * Compose with the slot subcomponents:
 *   <AppShell header={{ height: 56 }} navbar={{ width: 240, collapsed: { mobile: !open } }}>
 *     <AppShell.Header>...</AppShell.Header>
 *     <AppShell.Navbar>...</AppShell.Navbar>
 *     <AppShell.Main>...</AppShell.Main>
 *   </AppShell>
 */

export interface AppShellHeaderConfig {
    height?: number | string;
    /** Set false to disable the sticky behaviour. Default true. */
    sticky?: boolean;
}

export interface AppShellNavbarConfig {
    width?: number | string;
    /** Width while collapsed. Default 64. */
    collapsedWidth?: number | string;
    /** Tailwind-style breakpoint for mobile collapse — currently informational. */
    breakpoint?: "xs" | "sm" | "md" | "lg" | "xl";
    collapsed?: { mobile?: boolean; desktop?: boolean };
}

export interface AppShellProps extends Omit<HTMLAttributes<HTMLDivElement>, "color"> {
    header?: AppShellHeaderConfig;
    navbar?: AppShellNavbarConfig;
    padding?: string | number;
    w?: number | string;
    h?: number | string;
    p?: number | string;
    px?: number | string;
    py?: number | string;
    pt?: number | string;
    pb?: number | string;
    pl?: number | string;
    pr?: number | string;
    m?: number | string;
    mx?: number | string;
    my?: number | string;
    mt?: number | string;
    mb?: number | string;
    ml?: number | string;
    mr?: number | string;
    children?: ReactNode;
}

interface AppShellLayoutContextValue {
    headerHeight: string;
    navbarWidth: string;
    navbarCollapsed: boolean;
    headerSticky: boolean;
    contentPadding: string | number;
}

const AppShellLayoutContext = createContext<AppShellLayoutContextValue | undefined>(undefined);

const useAppShellLayout = () =>
    useContext(AppShellLayoutContext) ?? {
        headerHeight: "48px",
        navbarWidth: "240px",
        navbarCollapsed: false,
        headerSticky: true,
        contentPadding: 0,
    };

const sizeToCss = (s: number | string | undefined, fallback: string): string => {
    if (s === undefined) return fallback;
    return typeof s === "number" ? `${s}px` : s;
};

const AppShellRoot = forwardRef<HTMLDivElement, AppShellProps>(
    ({ header, navbar, padding, style, children, ...props }, ref) => {
        const headerHeight = sizeToCss(header?.height, "48px");
        const navbarCollapsed = Boolean(navbar?.collapsed?.mobile || navbar?.collapsed?.desktop);
        const navbarWidth = sizeToCss(
            navbarCollapsed ? (navbar?.collapsedWidth ?? 64) : navbar?.width,
            "240px",
        );
        const headerSticky = header?.sticky !== false;

        const ctx = useMemo<AppShellLayoutContextValue>(
            () => ({
                headerHeight,
                navbarWidth,
                navbarCollapsed,
                headerSticky,
                contentPadding: padding ?? 0,
            }),
            [headerHeight, navbarWidth, navbarCollapsed, headerSticky, padding],
        );

        const merged: CSSProperties = {
            display: "grid",
            gridTemplateColumns: `${navbarWidth} 1fr`,
            gridTemplateRows: `${headerHeight} 1fr`,
            gridTemplateAreas: `"header header" "navbar main"`,
            minHeight: "100vh",
            background: "var(--g-color-base-background)",
            color: "var(--g-color-text-primary)",
            transition: "grid-template-columns 200ms cubic-bezier(0.4, 0, 0.2, 1)",
            ...getSpacingStyles(props),
            ...style,
        };

        return (
            <AppShellLayoutContext.Provider value={ctx}>
                <div ref={ref} style={merged} {...cleanLayoutProps(props)}>
                    {children}
                </div>
            </AppShellLayoutContext.Provider>
        );
    },
);
AppShellRoot.displayName = "AppShell";

const AppShellHeader = forwardRef<HTMLElement, HTMLAttributes<HTMLElement>>(
    ({ style, ...props }, ref) => {
        const ctx = useAppShellLayout();
        const merged: CSSProperties = {
            gridArea: "header",
            position: ctx.headerSticky ? "sticky" : "static",
            top: 0,
            zIndex: 100,
            height: ctx.headerHeight,
            display: "flex",
            alignItems: "center",
            background: "var(--g-color-base-background)",
            borderBottom: "1px solid var(--g-color-line-generic)",
            ...style,
        };
        return <header ref={ref} style={merged} {...props} />;
    },
);
AppShellHeader.displayName = "AppShell.Header";

const AppShellNavbar = forwardRef<HTMLElement, HTMLAttributes<HTMLElement>>(
    ({ style, ...props }, ref) => {
        const ctx = useAppShellLayout();
        const merged: CSSProperties = {
            gridArea: "navbar",
            position: "sticky",
            top: ctx.headerHeight,
            alignSelf: "start",
            height: `calc(100vh - ${ctx.headerHeight})`,
            width: ctx.navbarWidth,
            overflowY: "auto",
            background: "var(--g-color-base-background)",
            borderRight: "1px solid var(--g-color-line-generic)",
            transition: "width 200ms cubic-bezier(0.4, 0, 0.2, 1)",
            ...style,
        };
        return <nav ref={ref} style={merged} {...props} />;
    },
);
AppShellNavbar.displayName = "AppShell.Navbar";

const AppShellMain = forwardRef<HTMLElement, HTMLAttributes<HTMLElement>>(
    ({ style, ...props }, ref) => {
        const ctx = useAppShellLayout();
        const merged: CSSProperties = {
            gridArea: "main",
            display: "flex",
            flexDirection: "column",
            minWidth: 0,
            padding: ctx.contentPadding,
            ...style,
        };
        return <main ref={ref} style={merged} {...props} />;
    },
);
AppShellMain.displayName = "AppShell.Main";

const AppShellAside = forwardRef<HTMLElement, HTMLAttributes<HTMLElement>>(
    ({ style, ...props }, ref) => {
        const merged: CSSProperties = {
            position: "sticky",
            top: 0,
            alignSelf: "start",
            height: "100vh",
            overflowY: "auto",
            borderLeft: "1px solid var(--g-color-line-generic)",
            background: "var(--g-color-base-background)",
            ...style,
        };
        return <aside ref={ref} style={merged} {...props} />;
    },
);
AppShellAside.displayName = "AppShell.Aside";

export const AppShell = Object.assign(AppShellRoot, {
    Header: AppShellHeader,
    Navbar: AppShellNavbar,
    Main: AppShellMain,
    Aside: AppShellAside,
});
