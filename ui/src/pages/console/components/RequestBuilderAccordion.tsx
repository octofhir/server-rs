import { Accordion, Group, Text, Badge } from "@mantine/core";
import { QueryBuilder } from "./QueryBuilder";
import { HeaderEditor } from "./HeaderEditor";
import { BodyEditor } from "./BodyEditor";
import { useUnit } from "effector-react";
import { $body, $customHeaders, $method, $queryParams, $searchParams } from "../state/consoleStore";
import type { RestConsoleSearchParam } from "@/shared/api";

interface RequestBuilderAccordionProps {
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	resourceType?: string;
	hideQuery?: boolean;
}

export function RequestBuilderAccordion({
	searchParamsByResource,
	resourceType,
	hideQuery = false,
}: RequestBuilderAccordionProps) {
	const { method, searchParams, queryParams, customHeaders, body } = useUnit({
		method: $method,
		searchParams: $searchParams,
		queryParams: $queryParams,
		customHeaders: $customHeaders,
		body: $body,
	});

	const paramCount = searchParams.length + Object.keys(queryParams).length;
	const customHeaderCount = Object.keys(customHeaders).length;
	const bodySize = body.length;

	const defaultValue = hideQuery ? ["headers", "body"] : ["query", "headers", "body"];

	return (
		<Accordion multiple defaultValue={defaultValue} variant="separated">
			{!hideQuery && (
				<Accordion.Item value="query">
					<Accordion.Control>
						<Group justify="space-between" w="100%">
							<Text size="sm">Query Parameters</Text>
							<Badge size="sm" variant="light">
								{paramCount} params
							</Badge>
						</Group>
					</Accordion.Control>
					<Accordion.Panel>
						<QueryBuilder
							searchParamsByResource={searchParamsByResource}
							resourceType={resourceType}
						/>
					</Accordion.Panel>
				</Accordion.Item>
			)}

			<Accordion.Item value="headers">
				<Accordion.Control>
					<Group justify="space-between" w="100%">
						<Text size="sm">Headers</Text>
						<Badge size="sm" variant="light">
							{customHeaderCount} custom
						</Badge>
					</Group>
				</Accordion.Control>
				<Accordion.Panel>
					<HeaderEditor />
				</Accordion.Panel>
			</Accordion.Item>

			<Accordion.Item value="body">
				<Accordion.Control>
					<Group justify="space-between" w="100%">
						<Text size="sm">Request Body</Text>
						<Badge size="sm" variant="light">
							{bodySize > 0 ? `${bodySize} bytes` : "Empty"}
						</Badge>
					</Group>
				</Accordion.Control>
				<Accordion.Panel>
					<BodyEditor resourceType={resourceType} method={method} />
				</Accordion.Panel>
			</Accordion.Item>
		</Accordion>
	);
}
