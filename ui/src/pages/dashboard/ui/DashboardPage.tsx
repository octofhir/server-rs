import { useNavigate } from "react-router-dom";
import { DashboardWorkspace } from "@/widgets/dashboard-workspace";
import classes from "./DashboardPage.module.css";

export function DashboardPage() {
	const navigate = useNavigate();

	return (
		<div className={classes.page}>
			<DashboardWorkspace onNavigate={navigate} />
		</div>
	);
}
