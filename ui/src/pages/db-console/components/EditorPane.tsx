import { useCallback, useEffect } from "react";
import { Group, Text, Button, ActionIcon, Tooltip, Popover, UnstyledButton, Box } from "@/shared/ui";
import { IconCode, IconSettings } from "@tabler/icons-react";
import { useDisclosure } from "@octofhir/ui-kit";
import type * as monaco from "monaco-editor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { DiagnosticsPanel } from "@/widgets/diagnostics-panel";
import { setLspFormatterConfig } from "@/shared/monaco/lspClient";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import { useFormatterSettings } from "@/shared/api/hooks";

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
	const [formatterOpened, { toggle: toggleFormatter, close: closeFormatter }] = useDisclosure(false);
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
		<Box style={{ flex: `0 0 ${editorHeight + 40}px`, display: "flex", flexDirection: "column", position: "relative" }}>
			{/* Toolbar */}
			<Group px="sm" py={4} justify="space-between" style={{ backgroundColor: "var(--octo-surface-2)", flexShrink: 0 }}>
				<Text size="xs" fw={500} c="dimmed">
					SQL Editor
				</Text>
				<Group gap={4}>
					<Tooltip label="Format SQL (Shift+Alt+F)">
						<ActionIcon variant="subtle" size="xs" onClick={handleFormat} disabled={!editorInstance}>
							<IconCode size={14} />
						</ActionIcon>
					</Tooltip>

					<Popover opened={formatterOpened} onClose={closeFormatter} position="bottom-end" width={320} shadow="md">
						<Popover.Target>
							<Tooltip label="Formatter Settings">
								<ActionIcon variant="subtle" size="xs" onClick={toggleFormatter}>
									<IconSettings size={14} />
								</ActionIcon>
							</Tooltip>
						</Popover.Target>
						<Popover.Dropdown>
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
							<Group justify="flex-end" mt="sm">
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
							</Group>
						</Popover.Dropdown>
					</Popover>

					<Button size="compact-xs" onClick={() => onExecute()} loading={isPending}>
						Execute
					</Button>
				</Group>
			</Group>

			{/* Editor */}
			<div style={{ height: `${editorHeight}px`, flexShrink: 0 }}>
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
				style={{
					height: "4px",
					cursor: "ns-resize",
					backgroundColor: "var(--octo-border-subtle)",
					transition: "background-color 0.15s",
					border: "none",
					padding: 0,
					flexShrink: 0,
				}}
				onMouseEnter={(e) => {
					e.currentTarget.style.backgroundColor = "var(--octo-accent-warm)";
				}}
				onMouseLeave={(e) => {
					e.currentTarget.style.backgroundColor = "var(--octo-border-subtle)";
				}}
			/>

			{/* Diagnostics */}
			<DiagnosticsPanel model={modelInstance} editor={editorInstance} defaultCollapsed height={140} />
		</Box>
	);
}
