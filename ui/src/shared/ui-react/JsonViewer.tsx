import { useMemo } from "react";
import { Code, CopyButton, ActionIcon, Tooltip, ScrollArea } from "@octofhir/ui-kit";
import { Copy, Check } from "lucide-react";
import classes from "../ui/JsonViewer/JsonViewer.module.css";

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
		<div className={`${classes.root} ${className || ""}`}>
			<div className={classes.copyButton}>
				<CopyButton value={formattedJson} timeout={2000}>
					{({ copied, copy }) => (
						<Tooltip label={copied ? "Copied to clipboard" : "Copy JSON"} withArrow position="left">
							<ActionIcon
								variant="light"
								color={copied ? "teal" : "primary"}
								onClick={copy}
								size="md"
								radius="md"
								className={classes.copyAction}
							>
								{copied ? <Check size={16} /> : <Copy size={16} />}
							</ActionIcon>
						</Tooltip>
					)}
				</CopyButton>
			</div>
			<ScrollArea h={maxHeight} type="auto">
				<div className={classes.content}>
					<Code block className={classes.code}>
						{formattedJson}
					</Code>
				</div>
			</ScrollArea>
			<div className={classes.indicator} />
		</div>
	);
}
