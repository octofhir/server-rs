import { useMemo } from "react";
import { Code, CopyButton, ActionIcon, Group, Tooltip, ScrollArea, Box } from "@mantine/core";
import { IconCopy, IconCheck } from "@tabler/icons-react";

interface JsonViewerProps {
	data: unknown;
	maxHeight?: number | string;
	className?: string;
}

/**
 * JSON viewer component with syntax highlighting and copy functionality.
 */
export function JsonViewer({ data, maxHeight = 400, className }: JsonViewerProps) {
	const formattedJson = useMemo(() => {
		try {
			return JSON.stringify(data, null, 2);
		} catch {
			return String(data);
		}
	}, [data]);

	return (
		<Box className={className} pos="relative">
			<Group pos="absolute" top={8} right={8} style={{ zIndex: 1 }}>
				<CopyButton value={formattedJson}>
					{({ copied, copy }) => (
						<Tooltip label={copied ? "Copied" : "Copy JSON"}>
							<ActionIcon
								variant="subtle"
								color={copied ? "teal" : "gray"}
								onClick={copy}
								size="sm"
							>
								{copied ? <IconCheck size={14} /> : <IconCopy size={14} />}
							</ActionIcon>
						</Tooltip>
					)}
				</CopyButton>
			</Group>
			<ScrollArea h={maxHeight} type="auto">
				<Code block style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
					{formattedJson}
				</Code>
			</ScrollArea>
		</Box>
	);
}

