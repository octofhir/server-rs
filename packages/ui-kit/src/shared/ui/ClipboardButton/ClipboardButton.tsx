import { type MouseEvent, type ReactNode, useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button, type ButtonProps } from "../Button";
import { Tooltip } from "../Tooltip";

export interface ClipboardButtonProps {
    /** Text written to the clipboard on click. */
    text: string;
    variant?: ButtonProps["variant"];
    size?: ButtonProps["size"];
    className?: string;
    /** Tooltip shown before copying. */
    tooltipInitialText?: ReactNode;
    /** Tooltip shown briefly after a successful copy. */
    tooltipSuccessText?: ReactNode;
    /** Optional label; when omitted the button is icon-only. */
    children?: ReactNode;
    disabled?: boolean;
    onClick?: (event: MouseEvent<HTMLButtonElement>) => void;
    "aria-label"?: string;
}

export function ClipboardButton({
    text,
    variant = "subtle",
    size = "sm",
    className,
    tooltipInitialText,
    tooltipSuccessText = "Copied!",
    children,
    disabled,
    onClick,
    "aria-label": ariaLabel,
}: ClipboardButtonProps) {
    const [copied, setCopied] = useState(false);

    const handleClick = async (event: MouseEvent<HTMLButtonElement | HTMLAnchorElement>) => {
        onClick?.(event as MouseEvent<HTMLButtonElement>);
        try {
            await navigator.clipboard.writeText(text);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        } catch {
            // Clipboard unavailable (e.g. insecure context) — fail silently.
        }
    };

    const button = (
        <Button
            variant={variant}
            size={size}
            disabled={disabled}
            className={className}
            aria-label={ariaLabel ?? "Copy to clipboard"}
            leftSection={copied ? <Check size={16} /> : <Copy size={16} />}
            onClick={(event) => void handleClick(event)}
        >
            {children}
        </Button>
    );

    const tip = copied ? tooltipSuccessText : tooltipInitialText;
    return tip ? <Tooltip content={tip}>{button}</Tooltip> : button;
}
