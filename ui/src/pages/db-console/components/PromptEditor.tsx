import { useCallback, useEffect, useState } from "react";
import { Group, Text, Button, ActionIcon, Tooltip, Popover, Box } from "@/shared/ui";
import { IconCode, IconSettings } from "@tabler/icons-react";
import { useDisclosure } from "@octofhir/ui-kit";
import type * as monaco from "monaco-editor";
import { SqlEditor } from "@/shared/monaco/SqlEditor";
import { DiagnosticsPanel } from "@/widgets/diagnostics-panel";
import { setLspFormatterConfig } from "@/shared/monaco/lspClient";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import { useFormatterSettings } from "@/shared/api/hooks";
import classes from "../DbConsolePage.module.css";

interface PromptEditorProps {
	initialQuery: string;
	onQueryChange: (value: string) => void;
	onExecute: (value?: string) => void;
	onEditorMount: (
		editor: monaco.editor.IStandaloneCodeEditor,
		model: monaco.editor.ITextModel,
	) => void;
	editorInstance: monaco.editor.IStandaloneCodeEditor | null;
	modelInstance: monaco.editor.ITextModel | null;
	isPending: boolean;
}

const MIN_HEIGHT = 60;
const MAX_HEIGHT = 300;
const LINE_HEIGHT = 20;

export function PromptEditor({
	initialQuery,
	onQueryChange,
	onExecute,
	onEditorMount,
	editorInstance,
	modelInstance,
	isPending,
}: PromptEditorProps) {
	const [formatterOpened, { toggle: toggleFormatter, close: closeFormatter }] =
		useDisclosure(false);
	const { config: formatterConfig, saveConfig: saveFormatterConfig } =
		useFormatterSettings();
	const [editorHeight, setEditorHeight] = useState(MIN_HEIGHT);

	useEffect(() => {
		setLspFormatterConfig(formatterConfig);
	}, [formatterConfig]);

	const handleFormat = useCallback(() => {
		editorInstance?.getAction("editor.action.formatDocument")?.run();
	}, [editorInstance]);

	// Auto-grow: listen to editor content changes and adjust height
	useEffect(() => {
		if (!editorInstance) return;

		const disposable = editorInstance.onDidChangeModelContent(() => {
			const lineCount = editorInstance.getModel()?.getLineCount() ?? 1;
			const contentHeight = Math.max(
				MIN_HEIGHT,
				Math.min(MAX_HEIGHT, lineCount * LINE_HEIGHT + 24),
			);
			setEditorHeight(contentHeight);
		});

		// Set initial height
		const lineCount = editorInstance.getModel()?.getLineCount() ?? 1;
		setEditorHeight(
			Math.max(MIN_HEIGHT, Math.min(MAX_HEIGHT, lineCount * LINE_HEIGHT + 24)),
		);

		return () => disposable.dispose();
	}, [editorInstance]);

	return (
		<div className={classes.promptContainer}>
			{/* Toolbar */}
			<div className={classes.promptToolbar}>
				<Group gap={6}>
					<Text size="xs" fw={600} c="dimmed">
						SQL
					</Text>
				</Group>
				<Group gap={4}>
					<Tooltip label="Format SQL (Shift+Alt+F)">
						<ActionIcon
							variant="subtle"
							size="xs"
							onClick={handleFormat}
							disabled={!editorInstance}
						>
							<IconCode size={14} />
						</ActionIcon>
					</Tooltip>

					<Popover
						opened={formatterOpened}
						onClose={closeFormatter}
						position="top-end"
						width={320}
						shadow="md"
					>
						<Popover.Target>
							<Tooltip label="Formatter Settings">
								<ActionIcon
									variant="subtle"
									size="xs"
									onClick={toggleFormatter}
								>
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

					<Button
						size="compact-xs"
						onClick={() => onExecute()}
						loading={isPending}
					>
						Execute
						<Text span size="xs" c="dimmed" ml={6}>
							Ctrl+↩
						</Text>
					</Button>
				</Group>
			</div>

			{/* Editor with prompt glyph */}
			<div className={classes.promptBody} style={{ height: editorHeight + 8 }}>
				<div className={classes.promptGlyph}>{`>>>`}</div>
				<div className={classes.promptEditorWrap}>
					<SqlEditor
						defaultValue={initialQuery}
						onChange={onQueryChange}
						onExecute={onExecute}
						onEditorMount={onEditorMount}
						enableLsp
					/>
				</div>
			</div>

			{/* Diagnostics */}
			<Box px="xs" pb="xs">
				<DiagnosticsPanel
					model={modelInstance}
					editor={editorInstance}
					defaultCollapsed
					height={120}
				/>
			</Box>
		</div>
	);
}
