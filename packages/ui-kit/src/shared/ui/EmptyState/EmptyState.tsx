import { forwardRef, isValidElement, type ReactNode } from "react";
import { Button, type ButtonProps } from "../Button";
import type { Size } from "../layout-utils";
import styles from "./EmptyState.module.css";

export interface EmptyStateAction {
    text: ReactNode;
    onClick?: () => void;
    variant?: ButtonProps["variant"];
    color?: ButtonProps["color"];
    icon?: ReactNode;
    disabled?: boolean;
}

export interface EmptyStateProps {
    /** Decorative element, or an image source descriptor. */
    image?: ReactNode | { src: string; alt?: string };
    title?: ReactNode;
    description?: ReactNode;
    /** Call-to-action buttons, or descriptors rendered as buttons. */
    actions?: ReactNode | EmptyStateAction[];
    /** `promo` adds extra vertical padding for full-page states. */
    size?: Size | "promo";
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
    { image, title, description, actions, size = "md", className, style },
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
            {actions != null && (
                <div className={styles.actions}>
                    {Array.isArray(actions)
                        ? actions.map((action, i) => (
                              <Button
                                  key={typeof action.text === "string" ? action.text : i}
                                  variant={action.variant ?? "filled"}
                                  color={action.color}
                                  onClick={action.onClick}
                                  disabled={action.disabled}
                                  leftSection={action.icon}
                              >
                                  {action.text}
                              </Button>
                          ))
                        : actions}
                </div>
            )}
        </div>
    );
});
