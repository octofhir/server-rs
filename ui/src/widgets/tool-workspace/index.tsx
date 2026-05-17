import type { ReactNode } from "react";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import type { WorkspacePageLayoutProps } from "@/widgets/workspace-page";
import classes from "./ToolWorkspaceLayout.module.css";

type ToolWorkspaceLayoutProps = Omit<
	WorkspacePageLayoutProps,
	"bodyClassName" | "contentClassName" | "children"
> & {
	children: ReactNode;
};

export function ToolWorkspaceLayout({
	children,
	className,
	...props
}: ToolWorkspaceLayoutProps) {
	return (
		<WorkspacePageLayout
			{...props}
			className={className}
			bodyClassName={classes.body}
			contentClassName={classes.content}
		>
			<div className={classes.main}>{children}</div>
		</WorkspacePageLayout>
	);
}
