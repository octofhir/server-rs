import type * as Monaco from "monaco-editor";

let nextId = 0;

/**
 * Binds a key in this editor only. `editor.addCommand` registers the key for every
 * editor on the page, and the last one registered wins: after visiting FHIRPath, Enter
 * in the SQL console ran the FHIRPath handler and stopped accepting completions.
 * Actions are scoped to their editor (`editorId == …`) and disposed with it.
 */
export function bindEditorKey(
  editor: Monaco.editor.IStandaloneCodeEditor,
  keybinding: number,
  label: string,
  run: () => void,
  when?: string
): Monaco.IDisposable {
  nextId += 1;
  return editor.addAction({
    id: `octofhir.key.${nextId}`,
    label,
    keybindings: [keybinding],
    keybindingContext: when,
    run: () => run(),
  });
}
