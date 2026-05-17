import { Badge, Box, Collapse, Flex, Text } from "@/shared/ui";
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
		<Box className={classes.panel}>
			<Flex
				justifyContent="space-between"
				alignItems="center"
				onClick={() => setExpanded(!expanded)}
				className={classes.collapsibleHeader}
			>
				<Flex gap="2" alignItems="center">
					{expanded ? (
						<ChevronDown size={16} />
					) : (
						<ChevronRight size={16} />
					)}
					<Text fw={500}>Metadata</Text>
				</Flex>
				<Flex gap="2">
					<Badge size="xs" color="blue">
						{metadata.timing.totalTime.toFixed(2)}ms
					</Badge>
				</Flex>
			</Flex>

			<Collapse in={expanded}>
				<Flex direction="column" gap="2" className={classes.metadataBody}>
					<Flex gap="2" alignItems="center">
						<Text size="sm" c="dimmed">
							Evaluator:
						</Text>
						<Text size="sm" ff="monospace">
							{metadata.evaluator}
						</Text>
					</Flex>

					<Flex gap="2" alignItems="center">
						<Text size="sm" c="dimmed">
							Result Count:
						</Text>
						<Badge size="sm" color="grape">
							{metadata.resultCount}
						</Badge>
					</Flex>

					<Flex gap="2" alignItems="center" wrap="wrap">
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
					</Flex>
				</Flex>
			</Collapse>
		</Box>
	);
}
