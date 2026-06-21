import type { ReactNode } from "react";
import { Modal as KitModal } from "@octofhir/ui-kit";

type ModalSize = "xs" | "sm" | "md" | "lg" | "xl" | "auto";

export interface ModalProps {
	children: ReactNode;
	onClose?: () => void;
	open?: boolean;
	opened?: boolean;
	title?: ReactNode;
	size?: ModalSize;
	footer?: ReactNode;
	hasCloseButton?: boolean;
	className?: string;
	/** @deprecated kept for source compatibility; the dialog always dismisses on outside click. */
	closeOnClickOutside?: boolean;
	/** @deprecated kept for source compatibility; the dialog always dismisses on Escape. */
	closeOnEscape?: boolean;
}

export function Modal({
	children,
	onClose,
	open,
	opened,
	title,
	size,
	footer,
	hasCloseButton = true,
	className,
}: ModalProps) {
	return (
		<KitModal
			open={open}
			opened={opened}
			onClose={onClose}
			title={title}
			size={size}
			footer={footer}
			withCloseButton={hasCloseButton}
			className={className}
		>
			{children}
		</KitModal>
	);
}
