import { forwardRef, type ReactNode } from "react";
import { Input as BaseInput } from "@base-ui/react/input";
import { Field } from "../Field/Field";
import type { Size } from "../layout-utils";
import styles from "../input.module.css";

export interface TextInputProps {
	value?: string;
	defaultValue?: string;
	onChange?: (value: string) => void;
	onUpdate?: (value: string) => void;
	placeholder?: string;
	disabled?: boolean;
	size?: Size;
	type?: string;
	name?: string;
	id?: string;
	autoFocus?: boolean;
	error?: boolean | string;
	label?: ReactNode;
	description?: ReactNode;
	required?: boolean;
	w?: number | string;
	leftSection?: ReactNode;
	rightSection?: ReactNode;
	startContent?: ReactNode;
	endContent?: ReactNode;
	className?: string;
	style?: React.CSSProperties;
	onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>;
	onBlur?: React.FocusEventHandler<HTMLInputElement>;
	onFocus?: React.FocusEventHandler<HTMLInputElement>;
	"aria-label"?: string;
}

export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(function TextInput(
	{
		value,
		defaultValue,
		onChange,
		onUpdate,
		size = "md",
		error,
		disabled,
		label,
		description,
		required,
		w,
		leftSection,
		rightSection,
		startContent,
		endContent,
		className,
		style,
		...props
	},
	ref,
) {
	const left = startContent ?? leftSection;
	const right = endContent ?? rightSection;
	return (
		<Field
			label={label}
			description={description}
			error={error}
			required={required}
			className={className}
			style={style}
			w={w}
		>
			<div
				className={styles.wrapper}
				data-size={size}
				data-error={error ? "true" : undefined}
				data-disabled={disabled ? "true" : undefined}
			>
				{left != null && <span className={styles.affix}>{left}</span>}
				<BaseInput
					ref={ref}
					className={styles.input}
					value={value}
					defaultValue={defaultValue}
					disabled={disabled}
					onValueChange={(next) => {
						onChange?.(next);
						onUpdate?.(next);
					}}
					{...props}
				/>
				{right != null && <span className={styles.affix}>{right}</span>}
			</div>
		</Field>
	);
});
