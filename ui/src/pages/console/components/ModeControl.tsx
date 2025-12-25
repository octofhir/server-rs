import { memo } from "react";
import { SegmentedControl } from "@mantine/core";
import { useUnit } from "effector-react";
import { $mode, setMode } from "../state/consoleStore";
import type { ConsoleMode } from "../state/consoleStore";

const MODE_OPTIONS: Array<{ label: string; value: ConsoleMode }> = [
	{ label: "Smart", value: "smart" },
	{ label: "Raw", value: "raw" },
];

export const ModeControl = memo(function ModeControl() {
	const { mode, setMode: setModeEvent } = useUnit({ mode: $mode, setMode });

	return (
		<SegmentedControl
			value={mode}
			onChange={(value) => setModeEvent(value as ConsoleMode)}
			data={MODE_OPTIONS}
			size="sm"
		/>
	);
});
