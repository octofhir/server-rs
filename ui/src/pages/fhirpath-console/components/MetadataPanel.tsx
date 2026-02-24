import { Badge, Collapse, Group, Paper, Stack, Text } from "@/shared/ui";
import { IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { useState } from "react";
import type { FhirPathMetadata } from "../types";

interface Props {
	metadata: FhirPathMetadata;
}

export function MetadataPanel({ metadata }: Props) {
	const [expanded, setExpanded] = useState(true);

	return (
		<Paper withBorder p="sm">
			<Group
				justify="space-between"
				onClick={() => setExpanded(!expanded)}
				style={{ cursor: "pointer" }}
			>
				<Group gap="xs">
					{expanded ? (
						<IconChevronDown size={16} />
					) : (
						<IconChevronRight size={16} />
					)}
					<Text fw={500}>Metadata</Text>
				</Group>
				<Group gap="xs">
					<Badge size="xs" color="blue">
						{metadata.timing.totalTime.toFixed(2)}ms
					</Badge>
				</Group>
			</Group>

			<Collapse in={expanded}>
				<Stack gap="xs" mt="sm">
					<Group gap="xs">
						<Text size="sm" c="dimmed">
							Evaluator:
						</Text>
						<Text size="sm" ff="monospace">
							{metadata.evaluator}
						</Text>
					</Group>

					<Group gap="xs">
						<Text size="sm" c="dimmed">
							Result Count:
						</Text>
						<Badge size="sm" color="grape">
							{metadata.resultCount}
						</Badge>
					</Group>

					<Group gap="xs">
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
					</Group>
				</Stack>
			</Collapse>
		</Paper>
	);
}
