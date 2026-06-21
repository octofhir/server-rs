import type { ComponentType } from "react";
import { Boxes as Boxes3, Code, Layers as Cubes3Overlap, Database, Monitor as Display, FileText, Folder, Variable as FunctionIcon, Settings as Gear, Globe, Users as Persons, Activity as Pulse, Receipt, Shield, ClipboardList as SquareListUl, Terminal } from "lucide-react";

export type ConsoleModuleLane = "operate" | "build" | "govern";
export type ConsoleModuleStatus = "ready" | "watch" | "draft";

export interface ConsoleModule {
	id: string;
	title: string;
	description: string;
	href: string;
	icon: ComponentType<{ width?: number; height?: number }>;
	lane: ConsoleModuleLane;
	status: ConsoleModuleStatus;
	tags: string[];
}

export const consoleModules: ConsoleModule[] = [
	{
		id: "resources",
		title: "Resource browser",
		description: "Inspect bundles, resource history, and typed FHIR payloads.",
		href: "/resources",
		icon: Folder,
		lane: "operate",
		status: "ready",
		tags: ["FHIR", "search"],
	},
	{
		id: "console",
		title: "REST console",
		description: "Build and replay FHIR REST requests with saved history.",
		href: "/console",
		icon: Terminal,
		lane: "operate",
		status: "ready",
		tags: ["REST", "history"],
	},
	{
		id: "db-console",
		title: "DB console",
		description: "Investigate storage tables, execution plans, and active queries.",
		href: "/db-console",
		icon: Database,
		lane: "operate",
		status: "watch",
		tags: ["SQL", "ops"],
	},
	{
		id: "packages",
		title: "Packages",
		description: "Install IG packages and inspect canonical artifacts.",
		href: "/packages",
		icon: Boxes3,
		lane: "build",
		status: "ready",
		tags: ["IG", "canonicals"],
	},
	{
		id: "viewdefinition",
		title: "ViewDefinition",
		description: "Model SQL-on-FHIR projections with preview and validation.",
		href: "/viewdefinition",
		icon: SquareListUl,
		lane: "build",
		status: "draft",
		tags: ["SQL-on-FHIR", "views"],
	},
	{
		id: "graphql",
		title: "GraphQL",
		description: "Explore graph queries and schema-backed responses.",
		href: "/graphql",
		icon: Code,
		lane: "build",
		status: "ready",
		tags: ["GraphQL", "schema"],
	},
	{
		id: "fhirpath",
		title: "FHIRPath",
		description: "Run expressions against example resources and metadata.",
		href: "/fhirpath",
		icon: Terminal,
		lane: "build",
		status: "ready",
		tags: ["FHIRPath", "LSP"],
	},
	{
		id: "operations",
		title: "Operations",
		description: "Manage operation definitions and endpoint visibility.",
		href: "/operations",
		icon: FunctionIcon,
		lane: "govern",
		status: "ready",
		tags: ["API", "policy"],
	},
	{
		id: "apps",
		title: "Apps",
		description: "Register app integrations and custom operation surfaces.",
		href: "/apps",
		icon: Cubes3Overlap,
		lane: "govern",
		status: "draft",
		tags: ["apps", "SMART"],
	},
	{
		id: "identity",
		title: "Identity providers",
		description: "Configure external identity, clients, users, roles, and sessions.",
		href: "/auth/providers",
		icon: Globe,
		lane: "govern",
		status: "watch",
		tags: ["OIDC", "auth"],
	},
	{
		id: "users",
		title: "Users and roles",
		description: "Review users, roles, policies, and active sessions.",
		href: "/auth/users",
		icon: Persons,
		lane: "govern",
		status: "ready",
		tags: ["RBAC", "sessions"],
	},
	{
		id: "audit",
		title: "Audit trail",
		description: "Trace security events and operational activity.",
		href: "/audit",
		icon: Receipt,
		lane: "govern",
		status: "ready",
		tags: ["audit", "security"],
	},
	{
		id: "sessions",
		title: "Sessions",
		description: "Watch active auth sessions and revocation state.",
		href: "/auth/sessions",
		icon: Display,
		lane: "govern",
		status: "ready",
		tags: ["tokens", "access"],
	},
	{
		id: "logs",
		title: "System logs",
		description: "Review server activity, diagnostics, and alerts.",
		href: "/logs",
		icon: FileText,
		lane: "operate",
		status: "watch",
		tags: ["logs", "diagnostics"],
	},
	{
		id: "settings",
		title: "Settings",
		description: "Tune UI and server-facing console configuration.",
		href: "/settings",
		icon: Gear,
		lane: "govern",
		status: "ready",
		tags: ["config", "server"],
	},
	{
		id: "health",
		title: "Health monitor",
		description: "Keep the live server health signal visible in the workspace.",
		href: "/",
		icon: Pulse,
		lane: "operate",
		status: "ready",
		tags: ["health", "status"],
	},
	{
		id: "policies",
		title: "Access policies",
		description: "Control operation access with policy-backed authorization.",
		href: "/auth/policies",
		icon: Shield,
		lane: "govern",
		status: "watch",
		tags: ["policy", "security"],
	},
];

export const consoleModuleLaneLabels: Record<ConsoleModuleLane, string> = {
	operate: "Operate",
	build: "Build",
	govern: "Govern",
};

export const consoleModuleStatusLabels: Record<ConsoleModuleStatus, string> = {
	ready: "Ready",
	watch: "Watch",
	draft: "Draft",
};
