import { SegmentedControl } from "@/shared/ui";
import { useUnit } from "effector-react";
import { $mode, setMode, type ConsoleMode } from "../state/consoleStore";

const MODE_DATA = [
  { label: "Pro", value: "pro" },
  { label: "Builder", value: "builder" },
];

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
      onChange={(v) => setModeEvent(v as ConsoleMode)}
    />
  );
}
