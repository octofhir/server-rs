import { Modal } from "@octofhir/ui-kit";
import classes from "./ShortcutsDialog.module.css";

interface ShortcutsDialogProps {
  open: boolean;
  onClose: () => void;
  goTargets: Array<{ key: string; label: string }>;
}

const GENERAL: Array<[string[], string]> = [
  [["⌘", "K"], "Command palette"],
  [["?"], "This list"],
  [["["], "Collapse sidebar"],
  [["↑", "↓"], "Move in lists"],
  [["j", "k"], "Move in lists (vim)"],
  [["↵"], "Open the focused row"],
  [["⌘", "↵"], "Run in consoles"],
  [["Esc"], "Close dialogs"],
];

/** Keyboard shortcut reference, opened with `?`. */
export function ShortcutsDialog({ open, onClose, goTargets }: ShortcutsDialogProps) {
  return (
    <Modal open={open} onClose={onClose} title="Keyboard shortcuts" size="md">
      <div className={classes.columns}>
        <section>
          <h3 className={classes.heading}>General</h3>
          {GENERAL.map(([keys, label]) => (
            <Row key={label} keys={keys} label={label} />
          ))}
        </section>
        <section>
          <h3 className={classes.heading}>Go to</h3>
          <Row keys={["g", "h"]} label="Overview" />
          {goTargets.map((target) => (
            <Row key={target.key} keys={["g", target.key]} label={target.label} />
          ))}
        </section>
      </div>
    </Modal>
  );
}

function Row({ keys, label }: { keys: string[]; label: string }) {
  return (
    <div className={classes.row}>
      <span>{label}</span>
      <span className={classes.keys}>
        {keys.map((key, index) => (
          // Keys repeat in sequences such as `g g`.
          <kbd key={`${index}-${key}`} className={classes.kbd}>
            {key}
          </kbd>
        ))}
      </span>
    </div>
  );
}
