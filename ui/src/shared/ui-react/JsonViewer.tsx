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
		<Box
			className={className}
			pos="relative"
			style={{
				borderRadius: "var(--mantine-radius-md)",
				overflow: "hidden",
				border: "1px solid var(--app-border-subtle)",
				backgroundColor: "var(--app-surface-2)",
			}}
		>
			<Group pos="absolute" top={12} right={12} style={{ zIndex: 5 }}>
				<CopyButton value={formattedJson} timeout={2000}>
					{({ copied, copy }) => (
						<Tooltip label={copied ? "Copied to clipboard" : "Copy JSON"} withArrow position="left">
							<ActionIcon
								variant="light"
								color={copied ? "teal" : "primary"}
								onClick={copy}
								size="md"
								radius="md"
								style={{
									boxShadow: "var(--mantine-shadow-xs)",
									backdropFilter: "blur(4px)",
								}}
							>
								{copied ? <IconCheck size={16} /> : <IconCopy size={16} />}
							</ActionIcon>
						</Tooltip>
					)}
				</CopyButton>
			</Group>
			<ScrollArea h={maxHeight} type="auto">
				<Box p="md">
					<Code
						block
						style={{
							whiteSpace: "pre-wrap",
							wordBreak: "break-word",
							backgroundColor: "transparent",
							color: "var(--app-text-primary)",
							fontSize: "var(--mantine-font-size-sm)",
							lineHeight: 1.6,
							fontFamily: "var(--mantine-font-family-monospace)",
						}}
					>
						{formattedJson}
					</Code>
				</Box>
			</ScrollArea>
			<Box
				style={{
					position: "absolute",
					left: 0,
					top: 0,
					bottom: 0,
					width: 4,
					background: "var(--app-brand-gradient)",
					opacity: 0.6
				}}
			/>
		</Box>
	);
}

