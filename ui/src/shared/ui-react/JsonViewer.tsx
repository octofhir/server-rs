import { useMemo } from "react";
import { Code, CopyButton, ScrollArea } from "@octofhir/ui-kit";
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
				<CopyButton
					text={formattedJson}
					variant="light"
					size="md"
					tooltipInitialText="Copy JSON"
					tooltipSuccessText="Copied to clipboard"
					aria-label="Copy JSON"
					className={classes.copyAction}
				/>
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
