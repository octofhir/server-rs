import { Server, SquareListUl, Wrench } from "@gravity-ui/icons";
import type { ReactNode } from "react";
import { Badge, Code, Loader, Text } from "@/shared/ui";
import { useCapabilities } from "@/shared/api/hooks";
import { WorkspacePageLayout, WorkspacePageSection } from "@/widgets/workspace-page";
import classes from "./CapabilityStatementPage.module.css";

export function CapabilityStatementPage() {
	const { data, isLoading, error } = useCapabilities();
	const rest = data?.rest ?? [];
	const resources = rest.flatMap((item) => item.resource ?? []);
	const systemInteractions = rest.flatMap((item) => item.interaction ?? []);
	const formats = data?.format ?? [];

	return (
		<WorkspacePageLayout
			title="Capability Statement"
			description="FHIR server metadata, advertised formats, resources, and interactions"
			maxWidth={1280}
			meta={
				<div className={classes.metaBadges}>
					<Badge color="primary" variant="light">
						{data?.fhirVersion ? `FHIR ${data.fhirVersion}` : "FHIR metadata"}
					</Badge>
					{data?.status ? <Badge variant="light">{data.status}</Badge> : null}
					{data?.date ? <Badge variant="light">{data.date}</Badge> : null}
				</div>
			}
		>
			{isLoading ? (
				<div className={classes.loadingState}>
					<Loader size="sm" />
					<Text color="secondary">Loading capability statement...</Text>
				</div>
			) : error ? (
				<Text color="danger">
					{error instanceof Error ? error.message : "Failed to load capability statement"}
				</Text>
			) : data ? (
				<>
					<div className={classes.stats}>
						<Metric icon={<Server size={18} />} label="REST modes" value={rest.length} />
						<Metric icon={<SquareListUl size={18} />} label="Resources" value={resources.length} />
						<Metric icon={<Wrench size={18} />} label="System interactions" value={systemInteractions.length} />
					</div>

					<WorkspacePageSection title="Server">
						<div className={classes.definitionGrid}>
							<Definition label="Software" value={data.software?.name} />
							<Definition label="Version" value={data.software?.version} />
							<Definition label="Implementation" value={data.implementation?.description} />
							<Definition label="Endpoint" value={data.implementation?.url} />
							<Definition label="Formats" value={formats.join(", ")} />
							<Definition label="Publisher" value={data.publisher} />
						</div>
					</WorkspacePageSection>

					<WorkspacePageSection title="Resources">
						<div className={classes.resourceGrid}>
							{resources.map((resource) => (
								<div key={resource.type} className={classes.resourceItem}>
									<Text variant="subheader-1">{resource.type}</Text>
									<div className={classes.interactionList}>
										{(resource.interaction ?? []).map((interaction) => (
											<Badge key={interaction.code} size="xs" variant="light">
												{interaction.code}
											</Badge>
										))}
									</div>
								</div>
							))}
						</div>
					</WorkspacePageSection>

					<WorkspacePageSection title="Raw JSON">
						<Code className={classes.raw}>{JSON.stringify(data, null, 2)}</Code>
					</WorkspacePageSection>
				</>
			) : null}
		</WorkspacePageLayout>
	);
}

function Metric({
	icon,
	label,
	value,
}: {
	icon: ReactNode;
	label: string;
	value: number | string;
}) {
	return (
		<div className={classes.metric}>
			<div className={classes.metricHeader}>
				<span className={classes.metricIcon}>{icon}</span>
				<Text color="secondary" variant="body-1">
					{label}
				</Text>
			</div>
			<Text variant="header-1">{value}</Text>
		</div>
	);
}

function Definition({ label, value }: { label: string; value: string | undefined }) {
	return (
		<div className={classes.definition}>
			<Text color="secondary" variant="caption-1">
				{label}
			</Text>
			<Text>{value || "Not advertised"}</Text>
		</div>
	);
}
