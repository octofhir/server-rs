import { useEffect, useMemo, useState } from "react";
import { useUnit } from "effector-react";
import {
	ScrollArea,
	Text,
	Portal,
	Stack,
	Button,
	ActionIcon,
	Modal,
	TextInput,
	Box,
	Group,
	Paper,
} from "@/shared/ui";
import { IconPin, IconPinFilled, IconX, IconCode, IconSettings } from "@tabler/icons-react";
import { useLocation, useNavigate } from "react-router-dom";
import { DndContext, PointerSensor, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import classes from "./AppTabsBar.module.css";
import {
	SortableContext,
	horizontalListSortingStrategy,
	useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
	$activeTabId,
	$tabs,
	closeTab,
	reorderTabs,
	renameTab,
	setActiveTab,
	togglePinTab,
	type AppTab,
} from "@/shared/state/appTabsStore";

type SortableTabProps = {
	tab: AppTab;
	isActive: boolean;
	onActivate: (id: string, path: string) => void;
	onClose: (id: string) => void;
	onTogglePin: (id: string) => void;
	colorScheme: "light" | "dark";
	onContextMenu: (event: React.MouseEvent, tab: AppTab) => void;
};

export function AppTabsBar() {
	const { tabs, activeTabId, closeTab: closeTabEvent, setActiveTab: setActiveTabEvent } =
		useUnit({
			tabs: $tabs,
			activeTabId: $activeTabId,
			closeTab,
			setActiveTab,
		});
	const { togglePinTab: togglePinTabEvent, reorderTabs: reorderTabsEvent } = useUnit({
		togglePinTab,
		reorderTabs,
	});
	const renameTabEvent = useUnit(renameTab);
	const navigate = useNavigate();
	const location = useLocation();
	const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));
	const [tabMenu, setTabMenu] = useState<{ x: number; y: number; tab: AppTab } | null>(null);
	const [renameTarget, setRenameTarget] = useState<AppTab | null>(null);
	const [renameValue, setRenameValue] = useState("");

	const activeTab = useMemo(
		() => tabs.find((tab) => tab.id === activeTabId) ?? null,
		[tabs, activeTabId],
	);

	const orderedTabs = useMemo(() => {
		const pinned = tabs.filter((tab) => tab.pinned);
		const unpinned = tabs.filter((tab) => !tab.pinned);
		return [...pinned, ...unpinned];
	}, [tabs]);

	useEffect(() => {
		const normalizedPath =
			location.pathname === "/"
				? "/"
				: location.pathname.replace(/\/+$/, "");
		const match = tabs.find((tab) => tab.path === normalizedPath);
		if (match && match.id !== activeTabId) {
			setActiveTabEvent(match.id);
		}
		if (!match && activeTabId !== null) {
			setActiveTabEvent(null);
		}
	}, [tabs, location.pathname, activeTabId, setActiveTabEvent]);

	if (tabs.length === 0) {
		return null;
	}

	const handleActivate = (tabId: string, path: string) => {
		if (location.pathname !== path) {
			navigate(path);
		}
		setActiveTabEvent(tabId);
	};

	const handleClose = (tabId: string) => {
		const wasActive = tabId === activeTabId;
		const remainingTabs = orderedTabs.filter((tab) => tab.id !== tabId);

		closeTabEvent(tabId);

		if (wasActive) {
			const nextTab = remainingTabs[0] ?? null;
			if (nextTab) {
				navigate(nextTab.path);
				setActiveTabEvent(nextTab.id);
			} else if (activeTab?.path === location.pathname) {
				navigate("/");
			}
		}
	};

	const handleDragEnd = (event: DragEndEvent) => {
		const { active, over } = event;
		if (!over || active.id === over.id) return;
		const activeTab = orderedTabs.find((tab) => tab.id === active.id);
		const overTab = orderedTabs.find((tab) => tab.id === over.id);
		if (!activeTab || !overTab || activeTab.pinned !== overTab.pinned) return;

		const next = [...orderedTabs];
		const fromIndex = next.findIndex((tab) => tab.id === active.id);
		const toIndex = next.findIndex((tab) => tab.id === over.id);
		if (fromIndex === -1 || toIndex === -1) return;
		const [moved] = next.splice(fromIndex, 1);
		next.splice(toIndex, 0, moved);
		reorderTabsEvent({ orderedIds: next.map((tab) => tab.id) });
	};

	const handleTabContextMenu = (event: React.MouseEvent, tab: AppTab) => {
		event.preventDefault();
		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
		setTabMenu({ x: rect.left + rect.width / 2, y: rect.bottom + 6, tab });
	};

	const handleTabMenuClose = () => setTabMenu(null);

	const handleTabRename = () => {
		if (!tabMenu) return;
		setRenameTarget(tabMenu.tab);
		setRenameValue(tabMenu.tab.title);
		handleTabMenuClose();
	};

	const handleTabPinToggle = () => {
		if (!tabMenu) return;
		togglePinTabEvent(tabMenu.tab.id);
		handleTabMenuClose();
	};

	const handleTabClose = () => {
		if (!tabMenu) return;
		handleClose(tabMenu.tab.id);
		handleTabMenuClose();
	};

	const handleRenameSubmit = () => {
		if (!renameTarget) return;
		const next = renameValue.trim();
		if (next) {
			renameTabEvent({ id: renameTarget.id, title: next });
		}
		setRenameTarget(null);
		setRenameValue("");
	};

	return (
		<Paper p={0} className={classes.tabsRoot}>
			{tabMenu && (
				<Portal>
					<Box
						style={{
							position: "fixed",
							left: tabMenu.x,
							top: tabMenu.y,
							transform: "translateX(-50%)",
							zIndex: 9999,
							backgroundColor: "var(--app-surface-1)",
							border: "1px solid var(--app-border-subtle)",
							borderRadius: 12,
							boxShadow: "var(--mantine-shadow-lg)",
							padding: 4,
							minWidth: 160,
						}}
						onMouseLeave={handleTabMenuClose}
					>
						<Stack gap={1}>
							<Button variant="subtle" fullWidth size="xs" justify="flex-start" leftSection={<IconCode size={14} />} onClick={handleTabRename} h={28}>
								Rename tab
							</Button>
							<Button variant="subtle" fullWidth size="xs" justify="flex-start" leftSection={tabMenu.tab.pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />} onClick={handleTabPinToggle} h={28}>
								{tabMenu.tab.pinned ? "Unpin tab" : "Pin tab"}
							</Button>
							<Button variant="subtle" fullWidth size="xs" justify="flex-start" leftSection={<IconSettings size={14} />} onClick={handleTabRename} h={28}>
								Tab settings
							</Button>
							{tabMenu.tab.closeable && (
								<Button
									color="red"
									variant="subtle"
									fullWidth
									size="xs"
									justify="flex-start"
									leftSection={<IconX size={14} />}
									onClick={handleTabClose}
									h={28}
								>
									Close tab
								</Button>
							)}
						</Stack>
					</Box>
				</Portal>
			)}
			<Modal
				opened={renameTarget !== null}
				onClose={() => {
					setRenameTarget(null);
					setRenameValue("");
				}}
				title="Rename Tab"
				size="sm"
			>
				<TextInput
					value={renameValue}
					onChange={(event) => setRenameValue(event.currentTarget.value)}
					placeholder="Tab name"
					autoFocus
					onKeyDown={(event: React.KeyboardEvent) => {
						if (event.key === "Enter") handleRenameSubmit();
					}}
				/>
				<Group justify="flex-end" mt="md" gap="sm">
					<Button variant="subtle" color="gray" onClick={() => setRenameTarget(null)}>
						Cancel
					</Button>
					<Button onClick={handleRenameSubmit}>Save Changes</Button>
				</Group>
			</Modal>
			<DndContext sensors={sensors} onDragEnd={handleDragEnd}>
				<SortableContext items={orderedTabs.map((tab) => tab.id)} strategy={horizontalListSortingStrategy}>
					<ScrollArea type="never" style={{ flex: 1 }}>
						<Group gap={4} wrap="nowrap" px={8} style={{ height: "100%" }}>
							{orderedTabs.map((tab) => (
								<SortableTab
									key={tab.id}
									tab={tab}
									isActive={tab.id === activeTabId}
									onActivate={handleActivate}
									onClose={handleClose}
									onContextMenu={handleTabContextMenu}
								/>
							))}
						</Group>
					</ScrollArea>
				</SortableContext>
			</DndContext>
		</Paper>
	);
}

function SortableTab({
	tab,
	isActive,
	onActivate,
	onClose,
	onContextMenu,
}: Omit<SortableTabProps, "onTogglePin" | "colorScheme">) {
	const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
		useSortable({ id: tab.id });
	const style = {
		transform: CSS.Transform.toString(transform),
		transition,
		opacity: isDragging ? 0.6 : 1,
		zIndex: isDragging ? 100 : 1,
	};

	return (
		<Box
			ref={setNodeRef}
			onClick={() => onActivate(tab.id, tab.path)}
			onContextMenu={(event: React.MouseEvent) => onContextMenu(event, tab)}
			className={classes.tab}
			data-active={isActive}
			style={style}
			{...attributes}
			{...listeners}
		>
			<Group gap={6} wrap="nowrap">
				{tab.pinned && (
					<IconPinFilled
						size={10}
						style={{
							color: "var(--app-brand-primary)",
							opacity: 0.8
						}}
					/>
				)}
				<Text
					size="xs"
					fw={isActive ? 600 : 500}
					maw={160}
					lineClamp={1}
					c="inherit"
					style={{ transition: "color 150ms ease" }}
				>
					{tab.title}
				</Text>
				{tab.closeable && (
					<ActionIcon
						variant="transparent"
						size={16}
						className={classes.closeIcon}
						onClick={(event: React.MouseEvent) => {
							event.stopPropagation();
							onClose(tab.id);
						}}
					>
						<IconX size={10} />
					</ActionIcon>
				)}
			</Group>

			{isActive && <Box className={classes.indicator} />}
			{!isActive && !isDragging && <Box className={classes.tabSeparator} />}
		</Box>
	);
}
