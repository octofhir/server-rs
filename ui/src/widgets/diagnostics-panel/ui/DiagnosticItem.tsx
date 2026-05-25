import { Box, Group, Text, UnstyledButton } from "@octofhir/ui-kit";
import {
	CircleExclamation,
	TriangleExclamation,
	CircleInfo,
	Bulb,
} from "@gravity-ui/icons";
import type * as monaco from "monaco-editor";
import type { DiagnosticInfo } from "@/shared/monaco/lib/useLspDiagnostics";

interface DiagnosticItemProps {
	diagnostic: DiagnosticInfo;
	onClick?: () => void;
}

/**
 * Severity icon component
 */
function SeverityIcon({ severity }: { severity: monaco.MarkerSeverity }) {
	const size = 16;

	switch (severity) {
		case 8: // monaco.MarkerSeverity.Error
			return <CircleExclamation size={size} color="var(--g-color-text-danger)" />;
		case 4: // monaco.MarkerSeverity.Warning
			return <TriangleExclamation size={size} color="var(--g-color-text-warning)" />;
		case 2: // monaco.MarkerSeverity.Info
			return <CircleInfo size={size} color="var(--g-color-text-info)" />;
		case 1: // monaco.MarkerSeverity.Hint
			return <Bulb size={size} color="var(--g-color-text-secondary)" />;
		default:
			return <CircleInfo size={size} color="var(--g-color-text-secondary)" />;
	}
}

/**
 * Individual diagnostic item with click-to-navigate functionality
 */
export function DiagnosticItem({ diagnostic, onClick }: DiagnosticItemProps) {
	const handleClick = () => {
		onClick?.();
	};

	return (
		<UnstyledButton
			onClick={handleClick}
			style={{
				width: "100%",
				padding: "8px 12px",
				borderRadius: "var(--g-border-radius-s)",
				transition: "background-color 0.2s",
				cursor: onClick ? "pointer" : "default",
			}}
		>
			<Group gap="xs" wrap="nowrap" align="flex-start">
				<Box style={{ marginTop: 2 }}>
					<SeverityIcon severity={diagnostic.severity} />
				</Box>
				<Box style={{ flex: 1, minWidth: 0 }}>
					<Text size="sm" style={{ wordBreak: "break-word" }}>
						{diagnostic.message}
					</Text>
					<Group gap={8} mt={4}>
						<Text size="xs" c="dimmed">
							Line {diagnostic.startLineNumber}:{diagnostic.startColumn}
						</Text>
						{diagnostic.source && (
							<Text size="xs" c="dimmed">
								• {diagnostic.source}
							</Text>
						)}
						{diagnostic.code && (
							<Text
								size="xs"
								c="dimmed"
								style={{ fontFamily: "var(--octo-typography-mono)" }}
							>
								[{diagnostic.code}]
							</Text>
						)}
					</Group>
				</Box>
			</Group>
		</UnstyledButton>
	);
}
