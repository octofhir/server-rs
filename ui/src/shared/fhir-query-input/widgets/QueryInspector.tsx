import { useMemo } from "react";
import {
	Tabs,
	Text,
	Badge,
	Table,
} from "@octofhir/ui-kit";
import { Code as CodeIcon, MessageCircle as Comment, Cloud } from "lucide-react";
import type { QueryAst, Diagnostic, QueryInputMetadata } from "../core/types";
import { explainQuery, type ExplainItem } from "../core/explain";
import { diffSelfLink, type SelfLinkDiff } from "../core/self-link-diff";
import classes from "./QueryInspector.module.css";

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
					value="parsed"
					icon={<CodeIcon width={14} />}
				>
					<span className={classes.tabLabel}>
						Parsed
						{diagnostics.length > 0 && (
							<Badge
								size="sm"
								theme={errorCount > 0 ? "danger" : "warning"}
							>
								{errorCount + warnCount}
							</Badge>
						)}
					</span>
				</Tabs.Tab>
				<Tabs.Tab
					value="explain"
					icon={<Comment size={14} />}
				>
					Explain
				</Tabs.Tab>
				{response && (
					<Tabs.Tab
						value="response"
						icon={<Cloud size={14} />}
					>
						Response
					</Tabs.Tab>
				)}
			</Tabs.List>

			<div className={classes.panelBody}>
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
			</div>
		</Tabs>
	);
}

function ParsedTab({
	ast,
	diagnostics,
}: { ast: QueryAst; diagnostics: Diagnostic[] }) {
	return (
		<div className={classes.stackLarge}>
			<section>
				<Text variant="caption-1" color="secondary" className={classes.sectionLabelCompact}>
					PATH
				</Text>
				<div className={classes.wrapRow}>
					<Badge theme="info" size="md">
						{ast.path.kind}
					</Badge>
					{"resourceType" in ast.path && (
						<code className={classes.inlineCode}>{ast.path.resourceType}</code>
					)}
					{"id" in ast.path && ast.path.id && (
						<code className={classes.inlineCode}>{ast.path.id}</code>
					)}
					{"operation" in ast.path && (
						<code className={classes.inlineCode}>{ast.path.operation}</code>
					)}
				</div>
			</section>

			{ast.params.length > 0 && (
				<section>
					<Text variant="caption-1" color="secondary" className={classes.sectionLabel}>
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
										<div className={classes.row}>
											<code className={classes.boldCode}>{p.name}</code>
											{p.isSpecial && (
												<Badge size="sm" theme="warning">
													special
												</Badge>
											)}
										</div>
									</Table.Td>
									<Table.Td>
										{p.modifier ? (
											<code>:{p.modifier}</code>
										) : (
											<Text color="secondary">-</Text>
										)}
									</Table.Td>
									<Table.Td>
										<div className={classes.wrapRow}>
											{p.values.map((v, j) => (
												<span key={`${v.raw}-${j}`} className={classes.row}>
													{v.prefix && (
														<Badge size="sm" theme="info">
															{v.prefix}
														</Badge>
													)}
													<code className={classes.inlineCode}>{v.raw}</code>
												</span>
											))}
										</div>
									</Table.Td>
								</Table.Tr>
							))}
						</Table.Tbody>
					</Table>
				</section>
			)}

			{diagnostics.length > 0 && (
				<section>
					<Text variant="caption-1" color="secondary" className={classes.sectionLabel}>
						DIAGNOSTICS ({diagnostics.length})
					</Text>
					<div className={classes.stackSmall}>
						{diagnostics.map((d, i) => (
							<div
								key={`${d.code}-${d.span.start}-${i}`}
								className={classes.row}
							>
								<Badge
									size="sm"
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
							</div>
						))}
					</div>
				</section>
			)}
		</div>
	);
}

function ExplainTab({ items }: { items: ExplainItem[] }) {
	if (items.length === 0) {
		return (
			<div className={classes.emptyExplain}>
				<Text variant="body-1">Type a FHIR query to see explanation.</Text>
			</div>
		);
	}

	return (
		<div className={classes.explainList}>
			{items.map((item, i) => (
				<div
					key={`${item.label}-${i}`}
					className={classes.explainItem}
				>
					<Badge
						size="sm"
						theme={
							item.kind === "path"
								? "info"
								: item.kind === "special"
									? "warning"
									: "normal"
						}
						className={classes.explainBadge}
					>
						{item.kind}
					</Badge>
					<div className={classes.explainBody}>
						<code className={classes.boldCode}>{item.label}</code>
						<Text variant="caption-1" color="secondary" className={classes.description}>
							{item.description}
						</Text>
					</div>
				</div>
			))}
		</div>
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
		<div className={classes.stackLarge}>
			<div className={classes.row}>
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
			</div>

			{issues.length > 0 && (
				<section>
					<Text variant="caption-1" color="secondary" className={classes.sectionLabel}>
						OPERATION OUTCOME ({issues.length} issues)
					</Text>
					<div className={classes.stackSmall}>
						{issues.map((issue, i) => (
							<div key={`${issue.code}-${i}`} className={classes.row}>
								<Badge
									size="sm"
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
							</div>
						))}
					</div>
				</section>
			)}

			{selfLinkDiff && (
				<section>
					<Text variant="caption-1" color="secondary" className={classes.sectionLabel}>
						SELF-LINK DIFF
					</Text>
					<div className={classes.stackSmall}>
						{selfLinkDiff.added.map((a) => (
							<Text key={a} variant="caption-1" className={classes.diffAdded}>
								+ {a} (added by server)
							</Text>
						))}
						{selfLinkDiff.removed.map((r) => (
							<Text key={r} variant="caption-1" className={classes.diffRemoved}>
								- {r} (removed by server)
							</Text>
						))}
						{selfLinkDiff.modified.map((m) => (
							<Text key={m.param} variant="caption-1" className={classes.diffModified}>
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
					</div>
				</section>
			)}
		</div>
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
