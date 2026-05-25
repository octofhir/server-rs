import { SegmentedControl } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { $mode, setMode, type ConsoleMode } from "../state/consoleStore";

const MODE_DATA = [
  { label: "Pro", value: "pro" },
  { label: "Builder", value: "builder" },
] satisfies Array<{ label: string; value: ConsoleMode }>;

function isConsoleMode(value: string): value is ConsoleMode {
  return MODE_DATA.some((option) => option.value === value);
}

export function ModeControl() {
  const { mode, setMode: setModeEvent } = useUnit({
    mode: $mode,
    setMode,
  });

  return (
    <SegmentedControl
      size="s"
      options={MODE_DATA}
      value={mode}
      onChange={(value) => {
        if (isConsoleMode(value)) {
          setModeEvent(value);
        }
      }}
    />
  );
}
