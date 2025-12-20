import { useState, useEffect } from "react";
import * as monaco from "monaco-editor";

export interface DiagnosticInfo {
	severity: monaco.MarkerSeverity;
	message: string;
	startLineNumber: number;
	startColumn: number;
	endLineNumber: number;
	endColumn: number;
	source?: string;
	code?: string;
}

export interface DiagnosticsByLevel {
	errors: DiagnosticInfo[];
	warnings: DiagnosticInfo[];
	info: DiagnosticInfo[];
	hints: DiagnosticInfo[];
}

/**
 * Hook to track LSP diagnostics for a Monaco editor model
 * Listens to Monaco marker changes and provides diagnostics grouped by severity
 */
export function useLspDiagnostics(
	model: monaco.editor.ITextModel | null,
): DiagnosticsByLevel {
	const [diagnostics, setDiagnostics] = useState<DiagnosticsByLevel>({
		errors: [],
		warnings: [],
		info: [],
		hints: [],
	});

	useEffect(() => {
		if (!model) {
			setDiagnostics({ errors: [], warnings: [], info: [], hints: [] });
			return;
		}

		// Get initial markers
		const updateDiagnostics = () => {
			const markers = monaco.editor.getModelMarkers({ resource: model.uri });

			const grouped: DiagnosticsByLevel = {
				errors: [],
				warnings: [],
				info: [],
				hints: [],
			};

			for (const marker of markers) {
				const diagnostic: DiagnosticInfo = {
					severity: marker.severity,
					message: marker.message,
					startLineNumber: marker.startLineNumber,
					startColumn: marker.startColumn,
					endLineNumber: marker.endLineNumber,
					endColumn: marker.endColumn,
					source: marker.source,
					code: marker.code ? String(marker.code) : undefined,
				};

				switch (marker.severity) {
					case monaco.MarkerSeverity.Error:
						grouped.errors.push(diagnostic);
						break;
					case monaco.MarkerSeverity.Warning:
						grouped.warnings.push(diagnostic);
						break;
					case monaco.MarkerSeverity.Info:
						grouped.info.push(diagnostic);
						break;
					case monaco.MarkerSeverity.Hint:
						grouped.hints.push(diagnostic);
						break;
				}
			}

			setDiagnostics(grouped);
		};

		// Initial update
		updateDiagnostics();

		// Listen for marker changes
		const disposable = monaco.editor.onDidChangeMarkers((uris) => {
			if (uris.some((uri) => uri.toString() === model.uri.toString())) {
				updateDiagnostics();
			}
		});

		return () => {
			disposable.dispose();
		};
	}, [model]);

	return diagnostics;
}

/**
 * Get total count of diagnostics
 */
export function getDiagnosticsCount(diagnostics: DiagnosticsByLevel): number {
	return (
		diagnostics.errors.length +
		diagnostics.warnings.length +
		diagnostics.info.length +
		diagnostics.hints.length
	);
}

/**
 * Get severity icon and color for a diagnostic
 */
export function getDiagnosticDisplay(severity: monaco.MarkerSeverity): {
	icon: string;
	color: string;
	label: string;
} {
	switch (severity) {
		case monaco.MarkerSeverity.Error:
			return { icon: "❌", color: "red", label: "Error" };
		case monaco.MarkerSeverity.Warning:
			return { icon: "⚠️", color: "yellow", label: "Warning" };
		case monaco.MarkerSeverity.Info:
			return { icon: "ℹ️", color: "blue", label: "Info" };
		case monaco.MarkerSeverity.Hint:
			return { icon: "💡", color: "gray", label: "Hint" };
		default:
			return { icon: "•", color: "gray", label: "Message" };
	}
}
