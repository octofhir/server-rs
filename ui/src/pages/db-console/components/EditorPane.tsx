import { ActionIcon, Button, Popover, Text, Tooltip, UnstyledButton, useDisclosure } from "@octofhir/ui-kit";
import { useCallback, useEffect } from "react";
import { Code, Settings as Gear } from "lucide-react";
import type * as monaco from "monaco-editor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { DiagnosticsPanel } from "@/widgets/diagnostics-panel";
import { setLspFormatterConfig } from "@/shared/monaco/lspClient";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import { useFormatterSettings } from "@/shared/api/hooks";
import classes from "../DbConsolePage.module.css";

interface EditorPaneProps {
	initialQuery: string;
	editorHeight: number;
	onEditorHeightChange: (height: number) => void;
	onQueryChange: (value: string) => void;
	onExecute: (value?: string) => void;
	onEditorMount: (editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel) => void;
	editorInstance: monaco.editor.IStandaloneCodeEditor | null;
	modelInstance: monaco.editor.ITextModel | null;
	isPending: boolean;
}

export function EditorPane({
	initialQuery,
	editorHeight,
	onEditorHeightChange,
	onQueryChange,
	onExecute,
	onEditorMount,
	editorInstance,
	modelInstance,
	isPending,
}: EditorPaneProps) {
	const [formatterOpened, { open: openFormatter, close: closeFormatter }] = useDisclosure(false);
	const { config: formatterConfig, saveConfig: saveFormatterConfig } = useFormatterSettings();

	useEffect(() => {
		setLspFormatterConfig(formatterConfig);
	}, [formatterConfig]);

	const handleFormat = useCallback(() => {
		editorInstance?.getAction("editor.action.formatDocument")?.run();
	}, [editorInstance]);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent) => {
			e.preventDefault();
			const startY = e.clientY;
			const startHeight = editorHeight;

			const handleMouseMove = (moveEvent: MouseEvent) => {
				const delta = moveEvent.clientY - startY;
				const newHeight = Math.max(200, Math.min(800, startHeight + delta));
				onEditorHeightChange(newHeight);
			};

			const handleMouseUp = () => {
				document.removeEventListener("mousemove", handleMouseMove);
				document.removeEventListener("mouseup", handleMouseUp);
			};

			document.addEventListener("mousemove", handleMouseMove);
			document.addEventListener("mouseup", handleMouseUp);
		},
		[editorHeight, onEditorHeightChange],
	);

	return (
		<div className={classes.editorPane} style={{ flexBasis: editorHeight + 40 }}>
			{/* Toolbar */}
			<div className={classes.editorToolbar}>
				<Text size="xs" fw={500} c="dimmed">
					SQL Editor
				</Text>
				<div className={classes.editorActions}>
					<Tooltip label="Format SQL (Shift+Alt+F)">
						<ActionIcon variant="subtle" size="xs" onClick={handleFormat} disabled={!editorInstance}>
							<Code size={14} />
						</ActionIcon>
					</Tooltip>

					<Popover
						open={formatterOpened}
						onOpenChange={(open) => (open ? openFormatter() : closeFormatter())}
						placement="bottom-end"
						trigger="click"
						content={
							<div className={classes.formatterPopover}>
								<Text size="sm" fw={500} mb="sm">
									SQL Formatter Settings
								</Text>
								<FormatterSettings
									value={formatterConfig}
									onChange={(config) => {
										setLspFormatterConfig(config);
										saveFormatterConfig(config);
									}}
									compact
								/>
								<div className={classes.popoverActions}>
									<Button
										size="xs"
										variant="light"
										onClick={() => {
											closeFormatter();
											handleFormat();
										}}
									>
										Apply & Format
									</Button>
								</div>
							</div>
						}
					>
						<Tooltip label="Formatter Settings">
							<ActionIcon variant="subtle" size="xs">
								<Gear size={14} />
							</ActionIcon>
						</Tooltip>
					</Popover>

					<Button size="xs" onClick={() => onExecute()} loading={isPending}>
						Execute
					</Button>
				</div>
			</div>

			{/* Editor */}
			<div className={classes.editorSlot} style={{ height: editorHeight }}>
				<SqlEditor
					defaultValue={initialQuery}
					onChange={onQueryChange}
					onExecute={onExecute}
					onEditorMount={onEditorMount}
					enableLsp
				/>
			</div>

			{/* Resize handle */}
			<UnstyledButton
				aria-label="Resize SQL editor"
				onMouseDown={handleMouseDown}
				onKeyDown={(e) => {
					if (e.key === "ArrowUp") onEditorHeightChange(Math.max(200, editorHeight - 20));
					else if (e.key === "ArrowDown") onEditorHeightChange(Math.min(800, editorHeight + 20));
				}}
				className={classes.editorResizeHandle}
			/>

			{/* Diagnostics */}
			<DiagnosticsPanel model={modelInstance} editor={editorInstance} defaultCollapsed height={140} />
		</div>
	);
}
