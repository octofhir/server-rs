import { useMemo, useState, useEffect, useCallback, useRef } from "react";
import {
	FhirPathEditor,
	type ConstantInfo,
} from "@/shared/monaco/FhirPathEditor";
import type { ViewDefinitionConstant } from "../../lib/useViewDefinition";

interface FHIRPathInputProps {
	value: string;
	onChange: (value: string) => void;
	resourceType: string;
	constants?: ViewDefinitionConstant[];
	forEachContext?: string[];
	placeholder?: string;
	size?: "xs" | "sm" | "md";
	autoFocus?: boolean;
	onBlur?: () => void;
}

/**
 * FHIRPath expression input with LSP-powered autocompletion.
 *
 * Uses Monaco editor in single-line mode with FHIRPath LSP support for:
 * - Schema-aware property completions
 * - Function suggestions
 * - Constant variables
 * - Real-time diagnostics
 *
 * Performance: Uses local state + debounced parent updates to avoid lag.
 */
export function FHIRPathInput({
	value,
	onChange,
	resourceType,
	constants = [],
	forEachContext,
	placeholder = "FHIRPath expression",
	size = "xs",
	autoFocus = false,
	onBlur,
}: FHIRPathInputProps) {
	// Local state for immediate UI updates (prevents lag)
	const [localValue, setLocalValue] = useState(value);
	const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const onChangeRef = useRef(onChange);
	onChangeRef.current = onChange;

	// Sync local state when parent value changes externally
	useEffect(() => {
		setLocalValue(value);
	}, [value]);

	// Handle local changes with debounced parent notification
	const handleChange = useCallback((newValue: string) => {
		setLocalValue(newValue);

		// Clear previous debounce
		if (debounceRef.current) {
			clearTimeout(debounceRef.current);
		}

		// Debounce parent update (150ms is a good balance between responsiveness and performance)
		debounceRef.current = setTimeout(() => {
			onChangeRef.current(newValue);
		}, 150);
	}, []);

	// Flush pending changes on blur
	const handleBlur = useCallback(() => {
		if (debounceRef.current) {
			clearTimeout(debounceRef.current);
			debounceRef.current = null;
		}
		// Always sync to parent on blur
		if (localValue !== value) {
			onChangeRef.current(localValue);
		}
		onBlur?.();
	}, [localValue, value, onBlur]);

	// Cleanup on unmount
	useEffect(() => {
		return () => {
			if (debounceRef.current) {
				clearTimeout(debounceRef.current);
			}
		};
	}, []);

	// Convert ViewDefinitionConstant[] to LSP constant format
	const lspConstants = useMemo(() => {
		const result: Record<string, ConstantInfo> = {};
		for (const c of constants) {
			let typeName = "string";
			if (c.valueInteger !== undefined) typeName = "integer";
			else if (c.valueBoolean !== undefined) typeName = "boolean";
			else if (c.valueDecimal !== undefined) typeName = "decimal";

			result[c.name] = { typeName };
		}
		return result;
	}, [constants]);

	// Calculate height based on size prop
	const height = size === "xs" ? 28 : size === "sm" ? 32 : 36;

	return (
		<FhirPathEditor
			value={localValue}
			onChange={handleChange}
			resourceType={resourceType}
			constants={lspConstants}
			forEachContext={forEachContext}
			placeholder={placeholder}
			autoFocus={autoFocus}
			onBlur={handleBlur}
			enableLsp={true}
			height={height}
		/>
	);
}
