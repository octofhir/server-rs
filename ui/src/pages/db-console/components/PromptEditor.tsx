import { useCallback, useEffect, useRef } from "react";
import {
	Group,
	Text,
	Button,
	ActionIcon,
	Tooltip,
	Popover,
	Box,
	Select,
	UnstyledButton,
} from "@/shared/ui";
import { IconCode, IconGripVertical, IconSettings } from "@tabler/icons-react";
import { useDisclosure, useLocalStorage } from "@octofhir/ui-kit";
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
	resultLimit: string;
	onResultLimitChange: (value: string) => void;
	sqlTimeout: string;
	onSqlTimeoutChange: (value: string) => void;
	onExecute: (value?: string) => void;
	onEditorMount: (
		editor: monaco.editor.IStandaloneCodeEditor,
		model: monaco.editor.ITextModel,
	) => void;
	editorInstance: monaco.editor.IStandaloneCodeEditor | null;
	modelInstance: monaco.editor.ITextModel | null;
	isPending: boolean;
}

const RESULT_LIMIT_OPTIONS = [
	{ value: "50", label: "Limit 50" },
	{ value: "100", label: "Limit 100" },
	{ value: "200", label: "Limit 200" },
	{ value: "500", label: "Limit 500" },
	{ value: "1000", label: "Limit 1000" },
	{ value: "none", label: "No limit" },
];
const TIMEOUT_OPTIONS = [
	{ value: "10000", label: "10s timeout" },
	{ value: "30000", label: "30s timeout" },
	{ value: "60000", label: "60s timeout" },
	{ value: "120000", label: "120s timeout" },
];
const DEFAULT_LIMIT = "200";
const DEFAULT_TIMEOUT = "120000";
const DIAGNOSTICS_MIN = 24;
const DIAGNOSTICS_MAX = 60;

export function PromptEditor({
	initialQuery,
	onQueryChange,
	resultLimit,
	onResultLimitChange,
	sqlTimeout,
	onSqlTimeoutChange,
	onExecute,
	onEditorMount,
	editorInstance,
	modelInstance,
	isPending,
}: PromptEditorProps) {
	const [formatterOpened, { toggle: toggleFormatter, close: closeFormatter }] =
		useDisclosure(false);
	const workspaceRef = useRef<HTMLDivElement>(null);
	const [diagnosticsSize, setDiagnosticsSize] = useLocalStorage({
		key: "db-console-diagnostics-size",
		defaultValue: 38,
	});
	const { config: formatterConfig, saveConfig: saveFormatterConfig } =
		useFormatterSettings();

	useEffect(() => {
		setLspFormatterConfig(formatterConfig);
	}, [formatterConfig]);

	const handleFormat = useCallback(() => {
		editorInstance?.getAction("editor.action.formatDocument")?.run();
	}, [editorInstance]);

	const clampDiagnosticsSize = useCallback((value: number) => {
		return Math.max(DIAGNOSTICS_MIN, Math.min(DIAGNOSTICS_MAX, value));
	}, []);

	const handleSplitterMouseDown = useCallback(
		(event: React.MouseEvent) => {
			event.preventDefault();
			const workspace = workspaceRef.current;
			if (!workspace) return;

			const rect = workspace.getBoundingClientRect();
			const vertical = window.matchMedia("(max-width: 1400px)").matches;

			const handleMouseMove = (moveEvent: MouseEvent) => {
				const rawSize = vertical
					? ((rect.bottom - moveEvent.clientY) / rect.height) * 100
					: ((rect.right - moveEvent.clientX) / rect.width) * 100;
				setDiagnosticsSize(clampDiagnosticsSize(rawSize));
			};

			const handleMouseUp = () => {
				document.removeEventListener("mousemove", handleMouseMove);
				document.removeEventListener("mouseup", handleMouseUp);
				document.body.style.cursor = "";
				document.body.style.userSelect = "";
			};

			document.body.style.cursor = vertical ? "row-resize" : "col-resize";
			document.body.style.userSelect = "none";
			document.addEventListener("mousemove", handleMouseMove);
			document.addEventListener("mouseup", handleMouseUp);
		},
		[clampDiagnosticsSize, setDiagnosticsSize],
	);

	const handleSplitterKeyDown = useCallback(
		(event: React.KeyboardEvent<HTMLButtonElement>) => {
			const vertical = window.matchMedia("(max-width: 1400px)").matches;
			const increase = vertical ? event.key === "ArrowUp" : event.key === "ArrowLeft";
			const decrease =
				vertical ? event.key === "ArrowDown" : event.key === "ArrowRight";
			if (!increase && !decrease) return;

			event.preventDefault();
			setDiagnosticsSize((prev) =>
				clampDiagnosticsSize(prev + (increase ? 2 : -2)),
			);
		},
		[clampDiagnosticsSize, setDiagnosticsSize],
	);

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

					<Tooltip label="Auto-limit for SELECT/WITH queries without LIMIT">
						<Select
							size="xs"
							w={112}
							value={resultLimit}
							onChange={(value) => onResultLimitChange(value ?? DEFAULT_LIMIT)}
							data={RESULT_LIMIT_OPTIONS}
							allowDeselect={false}
							aria-label="SQL result limit"
						/>
					</Tooltip>

					<Tooltip label="Client-side timeout for SQL execution">
						<Select
							size="xs"
							w={126}
							value={sqlTimeout}
							onChange={(value) => onSqlTimeoutChange(value ?? DEFAULT_TIMEOUT)}
							data={TIMEOUT_OPTIONS}
							allowDeselect={false}
							aria-label="SQL timeout"
						/>
					</Tooltip>

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

			<div
				ref={workspaceRef}
				className={classes.promptWorkspace}
				style={{ "--diagnostics-size": `${diagnosticsSize}%` } as React.CSSProperties}
			>
				{/* Editor with prompt glyph */}
				<div className={classes.promptBody}>
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

				<UnstyledButton
					className={classes.promptSplitter}
					onMouseDown={handleSplitterMouseDown}
					onKeyDown={handleSplitterKeyDown}
					aria-label="Resize diagnostics panel"
				>
					<IconGripVertical size={14} />
				</UnstyledButton>

				{/* Diagnostics */}
				<Box className={classes.diagnosticsPane} style={{ flex: 1, minHeight: 0 }}>
					<DiagnosticsPanel
						model={modelInstance}
						editor={editorInstance}
						height="100%"
					/>
				</Box>
			</div>
		</div>
	);
}
