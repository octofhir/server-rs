import { Badge, Collapse, Text } from "@/shared/ui";
import { ChevronDown, ChevronRight } from "@gravity-ui/icons";
import { useState } from "react";
import type { FhirPathMetadata } from "../types";
import classes from "../FhirPathConsolePage.module.css";

interface Props {
	metadata: FhirPathMetadata;
}

export function MetadataPanel({ metadata }: Props) {
	const [expanded, setExpanded] = useState(true);

	return (
		<div className={classes.panel}>
			<div
				onClick={() => setExpanded(!expanded)}
				className={classes.collapsibleHeader}
			>
				<div className={classes.metadataHeaderTitle}>
					{expanded ? (
						<ChevronDown size={16} />
					) : (
						<ChevronRight size={16} />
					)}
					<Text fw={500}>Metadata</Text>
				</div>
				<div className={classes.metadataBadges}>
					<Badge size="xs" color="blue">
						{metadata.timing.totalTime.toFixed(2)}ms
					</Badge>
				</div>
			</div>

			<Collapse in={expanded}>
				<div className={classes.metadataBody}>
					<div className={classes.metadataRow}>
						<Text size="sm" c="dimmed">
							Evaluator:
						</Text>
						<Text size="sm" ff="monospace">
							{metadata.evaluator}
						</Text>
					</div>

					<div className={classes.metadataRow}>
						<Text size="sm" c="dimmed">
							Result Count:
						</Text>
						<Badge size="sm" color="grape">
							{metadata.resultCount}
						</Badge>
					</div>

					<div className={classes.metadataTiming}>
						<Text size="sm" c="dimmed">
							Timing:
						</Text>
						<Badge size="xs" variant="light">
							Parse: {metadata.timing.parseTime.toFixed(2)}ms
						</Badge>
						<Badge size="xs" variant="light">
							Eval: {metadata.timing.evaluationTime.toFixed(2)}ms
						</Badge>
						<Badge size="xs" color="blue">
							Total: {metadata.timing.totalTime.toFixed(2)}ms
						</Badge>
					</div>
				</div>
			</Collapse>
		</div>
	);
}
