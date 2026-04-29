import { forwardRef, useState, type ReactNode } from "react";
import {
    AsideHeader,
    type AsideHeaderProps,
    type MenuItem as AsideMenuItem,
} from "@gravity-ui/navigation";

// `@gravity-ui/navigation` ships CSS co-located with each component and pulls
// it in via the JS module graph (sideEffects). No global stylesheet to import.

export type TrackerNavItem = AsideMenuItem;
export type TrackerLayoutProps = Omit<AsideHeaderProps, "renderContent" | "pinned"> & {
    /** Initial pinned state (controlled fallback). Default `true`. */
    defaultPinned?: boolean;
    /** Controlled pinned state. */
    pinned?: boolean;
    /** Main page content. Receives layout helpers from `AsideHeader`. */
    children?: ReactNode;
};

/**
 * Tracker-style admin layout — wraps `@gravity-ui/navigation` `AsideHeader`.
 *
 * Provides the canonical Gravity admin shell: collapsible sidebar with logo +
 * menu items + footer slot, sticky header, and a content area. Use this as the
 * top-level layout for `/ui` routes.
 *
 * @example
 *   <TrackerLayout
 *     logo={{ text: "OctoFHIR", icon: LogoIcon, iconSize: 32, href: "/ui" }}
 *     menuItems={[
 *       { id: "dashboard", title: "Dashboard", icon: HouseIcon, current: true },
 *       { id: "patients", title: "Patients", icon: PersonsIcon },
 *     ]}
 *     renderFooter={({ isPinned }) => isPinned ? <UserMenu /> : <UserAvatar />}
 *   >
 *     <Outlet />
 *   </TrackerLayout>
 */
export const TrackerLayout = forwardRef<HTMLDivElement, TrackerLayoutProps>(
    function TrackerLayout(
        { defaultPinned = true, pinned, children, onChangePinned, ...rest },
        ref,
    ) {
        const [internalPinned, setInternalPinned] = useState(defaultPinned);
        const isControlled = pinned !== undefined;
        const effectivePinned = isControlled ? (pinned as boolean) : internalPinned;

        const handlePinnedChange = (next: boolean) => {
            if (!isControlled) setInternalPinned(next);
            onChangePinned?.(next);
        };

        return (
            <AsideHeader
                ref={ref}
                pinned={effectivePinned}
                onChangePinned={handlePinnedChange}
                renderContent={() => <>{children}</>}
                {...rest}
            />
        );
    },
);
