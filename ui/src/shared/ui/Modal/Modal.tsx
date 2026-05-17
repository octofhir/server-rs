import type { ReactNode } from "react";
import { Dialog, type DialogProps } from "@gravity-ui/uikit";

type LegacyModalSize = "xs" | "sm" | "md" | "lg" | "xl";

export interface ModalProps
	extends Omit<DialogProps, "children" | "onClose" | "open" | "size"> {
	children: ReactNode;
	onClose?: () => void;
	open?: boolean;
	opened?: boolean;
	title?: ReactNode;
	size?: DialogProps["size"] | LegacyModalSize;
	closeOnClickOutside?: boolean;
	closeOnEscape?: boolean;
}

export function Modal({
	children,
	onClose,
	open,
	opened,
	title,
	size,
	closeOnClickOutside,
	closeOnEscape,
	disableOutsideClick,
	disableEscapeKeyDown,
	hasCloseButton = true,
	...props
}: ModalProps) {
	const isOpen = open ?? opened ?? false;

	return (
		<Dialog
			{...props}
			open={isOpen}
			onClose={() => onClose?.()}
			size={mapModalSize(size)}
			hasCloseButton={hasCloseButton}
			disableOutsideClick={disableOutsideClick ?? closeOnClickOutside === false}
			disableEscapeKeyDown={disableEscapeKeyDown ?? closeOnEscape === false}
		>
			{title ? <Dialog.Header caption={title} /> : null}
			<Dialog.Body>{children}</Dialog.Body>
		</Dialog>
	);
}

function mapModalSize(size: ModalProps["size"]): DialogProps["size"] {
	if (size === "xs" || size === "sm") return "s";
	if (size === "lg" || size === "xl") return "l";
	if (size === "md") return "m";
	return size;
}
