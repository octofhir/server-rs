import { CircleAlert, Info, Lightbulb, TriangleAlert } from "lucide-react";
import type { DiagnosticInfo } from "@/shared/monaco/lib/useLspDiagnostics";
import classes from "../DbConsolePage.module.css";

interface DiagnosticsListProps {
  diagnostics: DiagnosticInfo[];
  onNavigate: (lineNumber: number, column: number) => void;
}

function SeverityIcon({ severity }: { severity: number }) {
  switch (severity) {
    case 8: // Error
      return <CircleAlert size={14} className={classes.diagIconError} />;
    case 4: // Warning
      return <TriangleAlert size={14} className={classes.diagIconWarn} />;
    case 1: // Hint
      return <Lightbulb size={14} className={classes.diagIconHint} />;
    default: // Info (2)
      return <Info size={14} className={classes.diagIconInfo} />;
  }
}

export function DiagnosticsList({ diagnostics, onNavigate }: DiagnosticsListProps) {
  if (diagnostics.length === 0) {
    return <div className={classes.diagEmpty}>No problems detected</div>;
  }

  return (
    <ul className={classes.diagList}>
      {diagnostics.map((d, index) => (
        <li key={`${index}-${d.startLineNumber}-${d.startColumn}-${d.message}`}>
          <button
            type="button"
            className={classes.diagRow}
            onClick={() => onNavigate(d.startLineNumber, d.startColumn)}
            title={d.message}
          >
            <span className={classes.diagIcon}>
              <SeverityIcon severity={d.severity} />
            </span>
            <span className={classes.diagMsg}>{d.message}</span>
            {d.code && <span className={classes.diagCode}>{d.code}</span>}
            {d.source && <span className={classes.diagSource}>{d.source}</span>}
            <span className={classes.diagLoc}>
              {d.startLineNumber}:{d.startColumn}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
