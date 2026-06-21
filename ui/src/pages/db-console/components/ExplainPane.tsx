import { Code, SegmentedControl, Text } from "@octofhir/ui-kit";
import { useState, useMemo } from "react";
import type { SqlResponse } from "@/shared/api/types";
import { ExplainVisualization } from "@/widgets/explain-visualization";
import classes from "../DbConsolePage.module.css";

interface ExplainPaneProps {
	data: SqlResponse | undefined;
	error: Error | null;
	isPending: boolean;
}

export function ExplainPane({ data, error, isPending }: ExplainPaneProps) {
	const [mode, setMode] = useState<string>("visual");

	const explainText = useMemo(() => {
		if (!data) return "";
		return data.rows.map((row) => String(row[0])).join("\n");
	}, [data]);

	if (isPending) {
		return (
			<Text c="dimmed" ta="center" py="xl" size="sm">
				Running EXPLAIN ANALYZE...
			</Text>
		);
	}

	if (error) {
		return (
			<Text c="dimmed" ta="center" py="xl" size="sm">
				EXPLAIN not available for this query
			</Text>
		);
	}

	if (!data || data.rowCount === 0) {
		return (
			<Text c="dimmed" ta="center" py="xl" size="sm">
				No execution plan available
			</Text>
		);
	}

	return (
		<div className={classes.explainPane}>
			<div className={classes.explainToolbar}>
				<SegmentedControl
					value={mode}
					onChange={setMode}
					size="xs"
					options={[
						{ label: "Visual", value: "visual" },
						{ label: "Raw", value: "raw" },
					]}
				/>
			</div>
			{mode === "visual" ? (
				<ExplainVisualization explainText={explainText} />
			) : (
				<Code block className={classes.explainCode}>
					{explainText}
				</Code>
			)}
		</div>
	);
}
