import { useMemo } from "react";
import {
	Tabs,
	Text,
	Stack,
	Group,
	Badge,
	Box,
	Code,
	Table,
} from "@/shared/ui";
import {
	IconCode,
	IconMessageCircle,
	IconCloudComputing,
} from "@tabler/icons-react";
import type { QueryAst, Diagnostic, QueryInputMetadata } from "../core/types";
import { explainQuery, type ExplainItem } from "../core/explain";
import { diffSelfLink, type SelfLinkDiff } from "../core/self-link-diff";

export interface QueryInspectorProps {
	ast: QueryAst;
	diagnostics: Diagnostic[];
	metadata: QueryInputMetadata;
	response?: {
		status?: number;
		statusText?: string;
		durationMs?: number;
		body?: unknown;
		requestPath?: string;
	};
}

export function QueryInspector({
	ast,
	diagnostics,
	metadata,
	response,
}: QueryInspectorProps) {
	const explainItems = useMemo(
		() => explainQuery(ast, metadata),
		[ast, metadata],
	);

	const selfLinkDiff = useMemo(() => {
		if (!response?.body || !response.requestPath) return null;
		const body = response.body as Record<string, unknown>;
		if (!body.link || !Array.isArray(body.link)) return null;
		const selfLink = (body.link as Array<{ relation: string; url: string }>).find(
			(l) => l.relation === "self",
		);
		if (!selfLink) return null;
		return diffSelfLink(response.requestPath, selfLink.url);
	}, [response]);

	const errorCount = diagnostics.filter((d) => d.severity === "error").length;
	const warnCount = diagnostics.filter((d) => d.severity === "warning").length;

	return (
		<Tabs defaultValue="explain" variant="outline" radius="md">
			<Tabs.List>
				<Tabs.Tab
					value="parsed"
					leftSection={<IconCode size={14} />}
					rightSection={
						diagnostics.length > 0 ? (
							<Badge
								size="xs"
								variant="filled"
								color={errorCount > 0 ? "red" : "yellow"}
								circle
							>
								{errorCount + warnCount}
							</Badge>
						) : null
					}
				>
					Parsed
				</Tabs.Tab>
				<Tabs.Tab
					value="explain"
					leftSection={<IconMessageCircle size={14} />}
				>
					Explain
				</Tabs.Tab>
				{response && (
					<Tabs.Tab
						value="response"
						leftSection={<IconCloudComputing size={14} />}
					>
						Response
					</Tabs.Tab>
				)}
			</Tabs.List>

			<Tabs.Panel value="parsed" pt="sm">
				<ParsedTab ast={ast} diagnostics={diagnostics} />
			</Tabs.Panel>

			<Tabs.Panel value="explain" pt="sm">
				<ExplainTab items={explainItems} />
			</Tabs.Panel>

			{response && (
				<Tabs.Panel value="response" pt="sm">
					<ResponseTab response={response} selfLinkDiff={selfLinkDiff} />
				</Tabs.Panel>
			)}
		</Tabs>
	);
}

function ParsedTab({
	ast,
	diagnostics,
}: { ast: QueryAst; diagnostics: Diagnostic[] }) {
	return (
		<Stack gap="sm">
			<Box>
				<Text size="xs" fw={600} c="dimmed" mb={4}>
					PATH
				</Text>
				<Group gap="xs">
					<Badge variant="light" size="sm" color="primary">
						{ast.path.kind}
					</Badge>
					{"resourceType" in ast.path && (
						<Code>{ast.path.resourceType}</Code>
					)}
					{"id" in ast.path && ast.path.id && (
						<Code>{ast.path.id}</Code>
					)}
					{"operation" in ast.path && (
						<Code>{ast.path.operation}</Code>
					)}
				</Group>
			</Box>

			{ast.params.length > 0 && (
				<Box>
					<Text size="xs" fw={600} c="dimmed" mb={4}>
						PARAMETERS ({ast.params.length})
					</Text>
					<Table.ScrollContainer minWidth={200}>
						<Table
							striped
							highlightOnHover
							withTableBorder={false}
							styles={{ table: { fontSize: "var(--mantine-font-size-xs)" } }}
						>
							<Table.Thead>
								<Table.Tr>
									<Table.Th>Name</Table.Th>
									<Table.Th>Modifier</Table.Th>
									<Table.Th>Values</Table.Th>
								</Table.Tr>
							</Table.Thead>
							<Table.Tbody>
								{ast.params.map((p, i) => (
									<Table.Tr key={`${p.name}-${i}`}>
										<Table.Td>
											<Group gap={4}>
												<Code>{p.name}</Code>
												{p.isSpecial && (
													<Badge size="xs" variant="light" color="orange">
														special
													</Badge>
												)}
											</Group>
										</Table.Td>
										<Table.Td>
											{p.modifier ? (
												<Code>:{p.modifier}</Code>
											) : (
												<Text c="dimmed" size="xs">
													-
												</Text>
											)}
										</Table.Td>
										<Table.Td>
											<Group gap={4}>
												{p.values.map((v, j) => (
													<Group key={`${v.raw}-${j}`} gap={2}>
														{v.prefix && (
															<Badge size="xs" variant="light" color="indigo">
																{v.prefix}
															</Badge>
														)}
														<Code>{v.raw}</Code>
													</Group>
												))}
											</Group>
										</Table.Td>
									</Table.Tr>
								))}
							</Table.Tbody>
						</Table>
					</Table.ScrollContainer>
				</Box>
			)}

			{diagnostics.length > 0 && (
				<Box>
					<Text size="xs" fw={600} c="dimmed" mb={4}>
						DIAGNOSTICS ({diagnostics.length})
					</Text>
					<Stack gap={2}>
						{diagnostics.map((d, i) => (
							<Group
								key={`${d.code}-${d.span.start}-${i}`}
								gap="xs"
								wrap="nowrap"
							>
								<Badge
									size="xs"
									variant="light"
									color={
										d.severity === "error"
											? "red"
											: d.severity === "warning"
												? "yellow"
												: "blue"
									}
								>
									{d.severity}
								</Badge>
								<Text size="xs">{d.message}</Text>
							</Group>
						))}
					</Stack>
				</Box>
			)}
		</Stack>
	);
}

function ExplainTab({ items }: { items: ExplainItem[] }) {
	if (items.length === 0) {
		return (
			<Text size="sm" c="dimmed">
				Type a FHIR query to see explanation
			</Text>
		);
	}

	return (
		<Stack gap="xs">
			{items.map((item, i) => (
				<Group
					key={`${item.label}-${i}`}
					gap="sm"
					wrap="nowrap"
					align="flex-start"
				>
					<Badge
						size="xs"
						variant="light"
						color={
							item.kind === "path"
								? "primary"
								: item.kind === "special"
									? "orange"
									: "teal"
						}
						style={{ flexShrink: 0, marginTop: 2 }}
					>
						{item.kind}
					</Badge>
					<Box>
						<Code style={{ fontSize: 11 }}>{item.label}</Code>
						<Text size="xs" c="dimmed" mt={2}>
							{item.description}
						</Text>
					</Box>
				</Group>
			))}
		</Stack>
	);
}

function ResponseTab({
	response,
	selfLinkDiff,
}: {
	response: NonNullable<QueryInspectorProps["response"]>;
	selfLinkDiff: SelfLinkDiff | null;
}) {
	const body = response.body as Record<string, unknown> | undefined;
	const issues = getOperationOutcomeIssues(body);

	return (
		<Stack gap="sm">
			<Group gap="sm">
				<Badge
					variant="filled"
					color={
						(response.status ?? 0) >= 200 && (response.status ?? 0) < 300
							? "green"
							: (response.status ?? 0) >= 400
								? "red"
								: "yellow"
					}
				>
					{response.status} {response.statusText}
				</Badge>
				{response.durationMs !== undefined && (
					<Text size="xs" c="dimmed">
						{response.durationMs}ms
					</Text>
				)}
			</Group>

			{issues.length > 0 && (
				<Box>
					<Text size="xs" fw={600} c="dimmed" mb={4}>
						OPERATION OUTCOME ({issues.length} issues)
					</Text>
					<Stack gap={2}>
						{issues.map((issue, i) => (
							<Group key={`${issue.code}-${i}`} gap="xs" wrap="nowrap">
								<Badge
									size="xs"
									variant="light"
									color={
										issue.severity === "error" || issue.severity === "fatal"
											? "red"
											: issue.severity === "warning"
												? "yellow"
												: "blue"
									}
								>
									{issue.severity}
								</Badge>
								<Text size="xs">
									{issue.diagnostics || issue.code}
								</Text>
							</Group>
						))}
					</Stack>
				</Box>
			)}

			{selfLinkDiff && (
				<Box>
					<Text size="xs" fw={600} c="dimmed" mb={4}>
						SELF-LINK DIFF
					</Text>
					<Stack gap={2}>
						{selfLinkDiff.added.map((a) => (
							<Text key={a} size="xs" c="green">
								+ {a} (added by server)
							</Text>
						))}
						{selfLinkDiff.removed.map((r) => (
							<Text key={r} size="xs" c="red">
								- {r} (removed by server)
							</Text>
						))}
						{selfLinkDiff.modified.map((m) => (
							<Text key={m.param} size="xs" c="yellow">
								~ {m.param}: {m.sent} → {m.received}
							</Text>
						))}
						{selfLinkDiff.added.length === 0 &&
							selfLinkDiff.removed.length === 0 &&
							selfLinkDiff.modified.length === 0 && (
								<Text size="xs" c="dimmed">
									No differences — server accepted query as-is
								</Text>
							)}
					</Stack>
				</Box>
			)}
		</Stack>
	);
}

interface OOIssue {
	severity: string;
	code: string;
	diagnostics?: string;
}

function getOperationOutcomeIssues(body: unknown): OOIssue[] {
	if (!body || typeof body !== "object") return [];
	const obj = body as Record<string, unknown>;
	if (obj.resourceType !== "OperationOutcome") return [];
	if (!Array.isArray(obj.issue)) return [];
	return obj.issue as OOIssue[];
}
