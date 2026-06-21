import { type ReactNode, useEffect, useState } from "react";
import { createPortal } from "react-dom";

export interface PortalProps {
    children?: ReactNode;
    /** Target container. Defaults to `document.body`. */
    container?: Element | DocumentFragment | null;
    /** Render children inline instead of portaling. */
    disabled?: boolean;
}

export function Portal({ children, container, disabled }: PortalProps) {
    const [mounted, setMounted] = useState(false);
    useEffect(() => setMounted(true), []);

    if (disabled) return <>{children}</>;
    if (!mounted) return null;
    return createPortal(children, container ?? document.body);
}
