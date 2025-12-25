import { Group, Text, Box, UnstyledButton, useMantineTheme } from "@mantine/core";
import {
	IconAlertCircle,
	IconAlertTriangle,
	IconInfoCircle,
	IconBulb,
} from "@tabler/icons-react";
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
	const theme = useMantineTheme();
	const size = 16;

	switch (severity) {
		case 8: // monaco.MarkerSeverity.Error
			return <IconAlertCircle size={size} color={theme.colors.fire[6]} />;
		case 4: // monaco.MarkerSeverity.Warning
			return <IconAlertTriangle size={size} color={theme.colors.warm[6]} />;
		case 2: // monaco.MarkerSeverity.Info
			return <IconInfoCircle size={size} color={theme.colors.primary[6]} />;
		case 1: // monaco.MarkerSeverity.Hint
			return <IconBulb size={size} color={theme.colors.deep[5]} />;
		default:
			return <IconInfoCircle size={size} color={theme.colors.deep[5]} />;
	}
}

/**
 * Individual diagnostic item with click-to-navigate functionality
 */
export function DiagnosticItem({ diagnostic, onClick }: DiagnosticItemProps) {
	const theme = useMantineTheme();

	const handleClick = () => {
		onClick?.();
	};

	return (
		<UnstyledButton
			onClick={handleClick}
			style={{
				width: "100%",
				padding: "8px 12px",
				borderRadius: theme.radius.sm,
				transition: "background-color 0.2s",
				cursor: onClick ? "pointer" : "default",
			}}
			styles={{
					root: {
						"&:hover": onClick
							? {
								backgroundColor: "var(--app-surface-3)",
							}
							: {},
					},
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
								style={{ fontFamily: "var(--mantine-font-family-monospace)" }}
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
