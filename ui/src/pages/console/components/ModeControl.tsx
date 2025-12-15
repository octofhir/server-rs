import { SegmentedControl } from "@mantine/core";
import { IconWand, IconCode } from "@tabler/icons-react";
import { useConsoleStore } from "../state/consoleStore";
import type { ConsoleMode } from "../state/consoleStore";

const MODE_OPTIONS: Array<{ label: string; value: ConsoleMode }> = [
	{ label: "Smart", value: "smart" },
	{ label: "Raw", value: "raw" },
];

export function ModeControl() {
	const mode = useConsoleStore((state) => state.mode);
	const setMode = useConsoleStore((state) => state.setMode);

	return (
		<SegmentedControl
			value={mode}
			onChange={(value) => setMode(value as ConsoleMode)}
			data={MODE_OPTIONS.map((option) => ({
				value: option.value,
				label: option.label,
			}))}
			size="sm"
		/>
	);
}
