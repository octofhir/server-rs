import { useMemo } from "react";
import {
	Tabs,
	Text,
	Stack,
	Flex,
	Badge,
	Box,
	Table,
} from "@/shared/ui";
import {
	Code as CodeIcon,
	Comment,
	Cloud,
} from "@gravity-ui/icons";
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
		<Tabs defaultValue="explain">
			<Tabs.List>
				<Tabs.Tab
					id="parsed"
					icon={<CodeIcon width={14} />}
				>
					<Flex gap="1" alignItems="center">
						Parsed
						{diagnostics.length > 0 && (
							<Badge
								size="s"
								theme={errorCount > 0 ? "danger" : "warning"}
							>
								{errorCount + warnCount}
							</Badge>
						)}
					</Flex>
				</Tabs.Tab>
				<Tabs.Tab
					id="explain"
					icon={<Comment size={14} />}
				>
					Explain
				</Tabs.Tab>
				{response && (
					<Tabs.Tab
						id="response"
						icon={<Cloud size={14} />}
					>
						Response
					</Tabs.Tab>
				)}
			</Tabs.List>

			<Box style={{ paddingTop: "16px" }}>
				<Tabs.Panel value="parsed">
					<ParsedTab ast={ast} diagnostics={diagnostics} />
				</Tabs.Panel>

				<Tabs.Panel value="explain">
					<ExplainTab items={explainItems} />
				</Tabs.Panel>

				{response && (
					<Tabs.Panel value="response">
						<ResponseTab response={response} selfLinkDiff={selfLinkDiff} />
					</Tabs.Panel>
				)}
			</Box>
		</Tabs>
	);
}

function ParsedTab({
	ast,
	diagnostics,
}: { ast: QueryAst; diagnostics: Diagnostic[] }) {
	return (
		<Stack gap="4">
			<Box>
				<Text variant="caption-1" color="secondary" style={{ marginBottom: "4px", fontWeight: 700 }}>
					PATH
				</Text>
				<Flex gap="2" wrap="wrap" alignItems="center">
					<Badge theme="info" size="m">
						{ast.path.kind}
					</Badge>
					{"resourceType" in ast.path && (
						<code style={{ backgroundColor: "var(--g-color-base-generic-subtle)", padding: "2px 4px", borderRadius: "4px" }}>{ast.path.resourceType}</code>
					)}
					{"id" in ast.path && ast.path.id && (
						<code style={{ backgroundColor: "var(--g-color-base-generic-subtle)", padding: "2px 4px", borderRadius: "4px" }}>{ast.path.id}</code>
					)}
					{"operation" in ast.path && (
						<code style={{ backgroundColor: "var(--g-color-base-generic-subtle)", padding: "2px 4px", borderRadius: "4px" }}>{ast.path.operation}</code>
					)}
				</Flex>
			</Box>

			{ast.params.length > 0 && (
				<Box>
					<Text variant="caption-1" color="secondary" style={{ marginBottom: "8px", fontWeight: 700 }}>
						PARAMETERS ({ast.params.length})
					</Text>
					<Table striped highlightOnHover>
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
										<Flex gap="2" alignItems="center">
											<code style={{ fontWeight: 700 }}>{p.name}</code>
											{p.isSpecial && (
												<Badge size="s" theme="warning">
													special
												</Badge>
											)}
										</Flex>
									</Table.Td>
									<Table.Td>
										{p.modifier ? (
											<code>:{p.modifier}</code>
										) : (
											<Text color="secondary">-</Text>
										)}
									</Table.Td>
									<Table.Td>
										<Flex gap="2" wrap="wrap">
											{p.values.map((v, j) => (
												<Flex key={`${v.raw}-${j}`} gap="1" alignItems="center">
													{v.prefix && (
														<Badge size="s" theme="info">
															{v.prefix}
														</Badge>
													)}
													<code style={{ backgroundColor: "var(--g-color-base-generic-subtle)", padding: "2px 4px", borderRadius: "4px" }}>{v.raw}</code>
												</Flex>
											))}
										</Flex>
									</Table.Td>
								</Table.Tr>
							))}
						</Table.Tbody>
					</Table>
				</Box>
			)}

			{diagnostics.length > 0 && (
				<Box>
					<Text variant="caption-1" color="secondary" style={{ marginBottom: "8px", fontWeight: 700 }}>
						DIAGNOSTICS ({diagnostics.length})
					</Text>
					<Stack gap="2">
						{diagnostics.map((d, i) => (
							<Flex
								key={`${d.code}-${d.span.start}-${i}`}
								gap="2"
								alignItems="center"
							>
								<Badge
									size="s"
									theme={
										d.severity === "error"
											? "danger"
											: d.severity === "warning"
												? "warning"
												: "info"
									}
								>
									{d.severity}
								</Badge>
								<Text variant="body-1">{d.message}</Text>
							</Flex>
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
			<Flex direction="column" alignItems="center" style={{ padding: "20px 0", opacity: 0.5 }}>
				<Text variant="body-1">Type a FHIR query to see explanation.</Text>
			</Flex>
		);
	}

	return (
		<Stack gap="3">
			{items.map((item, i) => (
				<Flex
					key={`${item.label}-${i}`}
					gap="3"
					alignItems="flex-start"
				>
					<Badge
						size="s"
						theme={
							item.kind === "path"
								? "info"
								: item.kind === "special"
									? "warning"
									: "normal"
						}
						style={{ flexShrink: 0, marginTop: "2px" }}
					>
						{item.kind}
					</Badge>
					<Box>
						<code style={{ fontWeight: 700 }}>{item.label}</code>
						<Text variant="caption-1" color="secondary" style={{ marginTop: "2px" }}>
							{item.description}
						</Text>
					</Box>
				</Flex>
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
		<Stack gap="4">
			<Flex gap="2" alignItems="center">
				<Badge
					theme={
						(response.status ?? 0) >= 200 && (response.status ?? 0) < 300
							? "success"
							: (response.status ?? 0) >= 400
								? "danger"
								: "warning"
					}
				>
					{response.status} {response.statusText}
				</Badge>
				{response.durationMs !== undefined && (
					<Text color="secondary" variant="caption-1">
						{response.durationMs}ms
					</Text>
				)}
			</Flex>

			{issues.length > 0 && (
				<Box>
					<Text variant="caption-1" color="secondary" style={{ marginBottom: "8px", fontWeight: 700 }}>
						OPERATION OUTCOME ({issues.length} issues)
					</Text>
					<Stack gap="2">
						{issues.map((issue, i) => (
							<Flex key={`${issue.code}-${i}`} gap="2" alignItems="center">
								<Badge
									size="s"
									theme={
										issue.severity === "error" || issue.severity === "fatal"
											? "danger"
											: issue.severity === "warning"
												? "warning"
												: "info"
									}
								>
									{issue.severity}
								</Badge>
								<Text variant="body-1">
									{issue.diagnostics || issue.code}
								</Text>
							</Flex>
						))}
					</Stack>
				</Box>
			)}

			{selfLinkDiff && (
				<Box>
					<Text variant="caption-1" color="secondary" style={{ marginBottom: "8px", fontWeight: 700 }}>
						SELF-LINK DIFF
					</Text>
					<Stack gap="2">
						{selfLinkDiff.added.map((a) => (
							<Text key={a} variant="caption-1" style={{ color: "var(--g-color-text-success)" }}>
								+ {a} (added by server)
							</Text>
						))}
						{selfLinkDiff.removed.map((r) => (
							<Text key={r} variant="caption-1" style={{ color: "var(--g-color-text-danger)" }}>
								- {r} (removed by server)
							</Text>
						))}
						{selfLinkDiff.modified.map((m) => (
							<Text key={m.param} variant="caption-1" style={{ color: "var(--g-color-text-warning)" }}>
								~ {m.param}: {m.sent} → {m.received}
							</Text>
						))}
						{selfLinkDiff.added.length === 0 &&
							selfLinkDiff.removed.length === 0 &&
							selfLinkDiff.modified.length === 0 && (
								<Text variant="caption-1" color="secondary">
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
