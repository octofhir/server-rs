import { forwardRef, isValidElement, type ReactNode } from "react";
import styles from "./EmptyState.module.css";

export interface EmptyStateProps {
    /** Decorative element, or an image source descriptor. */
    image?: ReactNode | { src: string; alt?: string };
    title?: ReactNode;
    description?: ReactNode;
    /** Call-to-action buttons. */
    actions?: ReactNode;
    /** `promo` adds extra vertical padding for full-page states. */
    size?: "s" | "m" | "l" | "promo";
    className?: string;
    style?: React.CSSProperties;
}

function renderImage(image: EmptyStateProps["image"]): ReactNode {
    if (image == null) return null;
    if (isValidElement(image)) return image;
    if (typeof image === "object" && "src" in image) {
        return <img src={image.src} alt={image.alt ?? ""} />;
    }
    return image as ReactNode;
}

export const EmptyState = forwardRef<HTMLDivElement, EmptyStateProps>(function EmptyState(
    { image, title, description, actions, size = "m", className, style },
    ref,
) {
    return (
        <div
            ref={ref}
            data-size={size}
            className={[styles.root, className].filter(Boolean).join(" ")}
            style={style}
        >
            {image != null && <div className={styles.image}>{renderImage(image)}</div>}
            {title != null && <h3 className={styles.title}>{title}</h3>}
            {description != null && <p className={styles.description}>{description}</p>}
            {actions != null && <div className={styles.actions}>{actions}</div>}
        </div>
    );
});
