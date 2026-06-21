import type { CSSProperties, ReactNode } from "react";
import { getSpacingStyles, type SpacingProps } from "../layout-utils";
import styles from "../input.module.css";

export interface FieldProps extends SpacingProps {
	/** Field label rendered above the control. */
	label?: ReactNode;
	/** Helper text rendered below the control. */
	description?: ReactNode;
	/** Error message; a string replaces the description and tints the control. */
	error?: boolean | string;
	/** Marks the field as required (adds an asterisk to the label). */
	required?: boolean;
	className?: string;
	style?: CSSProperties;
	children: ReactNode;
}

/**
 * Vertical field layout: optional label, the control, and a description or
 * error line. Shared by the kit's text/number/password/select inputs.
 */
export function Field({
	label,
	description,
	error,
	required,
	className,
	style,
	children,
	...spacing
}: FieldProps) {
	const errorText = typeof error === "string" && error.length > 0 ? error : undefined;
	return (
		<div
			className={[styles.field, className].filter(Boolean).join(" ")}
			style={{ ...getSpacingStyles(spacing), ...style }}
		>
			{label != null && (
				<label className={styles.label}>
					{label}
					{required && <span className={styles.required}>*</span>}
				</label>
			)}
			{children}
			{errorText ? (
				<span className={styles.errorText}>{errorText}</span>
			) : (
				description != null && <span className={styles.description}>{description}</span>
			)}
		</div>
	);
}
