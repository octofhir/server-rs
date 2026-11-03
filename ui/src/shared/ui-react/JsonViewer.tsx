import { type ReactNode, useMemo } from "react";
import { Code, CopyButton, ScrollArea } from "@octofhir/ui-kit";
import classes from "../ui/JsonViewer/JsonViewer.module.css";

interface JsonViewerProps {
	data: unknown;
	maxHeight?: number | string;
	className?: string;
}

/** Above this size the body renders as plain text: one span per token would stall the page. */
const HIGHLIGHT_LIMIT = 200_000;

/**
 * Tokens of pretty-printed JSON: a string (a key when followed by `:`), a number,
 * a literal, or structural punctuation. Whitespace is left between matches.
 */
const JSON_TOKEN = /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|([{}[\],])/g;

function highlightJson(json: string): ReactNode[] {
	const nodes: ReactNode[] = [];
	let last = 0;
	for (const match of json.matchAll(JSON_TOKEN)) {
		const start = match.index;
		if (start > last) nodes.push(json.slice(last, start));
		const [token, str, colon, num, literal] = match;
		if (str !== undefined) {
			nodes.push(
				<span key={start} className={colon ? classes.key : classes.string}>
					{str}
				</span>,
			);
			if (colon) {
				nodes.push(
					<span key={`${start}:`} className={classes.punctuation}>
						{colon}
					</span>,
				);
			}
		} else if (num !== undefined) {
			nodes.push(
				<span key={start} className={classes.number}>
					{num}
				</span>,
			);
		} else if (literal !== undefined) {
			nodes.push(
				<span key={start} className={classes.literal}>
					{literal}
				</span>,
			);
		} else {
			nodes.push(
				<span key={start} className={classes.punctuation}>
					{token}
				</span>,
			);
		}
		last = start + token.length;
	}
	if (last < json.length) nodes.push(json.slice(last));
	return nodes;
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

	const content = useMemo(
		() => (formattedJson.length > HIGHLIGHT_LIMIT ? formattedJson : highlightJson(formattedJson)),
		[formattedJson],
	);

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
						{content}
					</Code>
				</div>
			</ScrollArea>
		</div>
	);
}
