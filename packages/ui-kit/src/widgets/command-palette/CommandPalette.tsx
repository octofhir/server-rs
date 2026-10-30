import { Dialog } from "@base-ui/react/dialog";
import { Search } from "lucide-react";
import {
    createElement,
    isValidElement,
    useEffect,
    useMemo,
    useRef,
    useState,
    type ComponentType,
    type KeyboardEvent,
    type ReactNode,
} from "react";
import styles from "./CommandPalette.module.css";

type IconLike = ReactNode | ComponentType<{ size?: number }>;

export interface CommandPaletteItem {
    id: string;
    label: string;
    /** Group heading the item is listed under. */
    group?: string;
    icon?: IconLike;
    /** Extra words matched by the filter but not shown. */
    keywords?: string[];
    /** Right-aligned hint, e.g. a route or shortcut. */
    hint?: ReactNode;
    onSelect: () => void;
}

export interface CommandPaletteProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    items: CommandPaletteItem[];
    placeholder?: string;
    emptyLabel?: ReactNode;
}

function renderIcon(icon: IconLike | undefined): ReactNode {
    if (icon == null) return null;
    if (isValidElement(icon)) return icon;
    if (typeof icon === "function" || (typeof icon === "object" && "$$typeof" in icon)) {
        return createElement(icon as ComponentType<{ size?: number }>, { size: 15 });
    }
    return icon;
}

/** Subsequence match scored by how early and how contiguous the hits are; null = no match. */
function score(query: string, item: CommandPaletteItem): number | null {
    if (!query) return 0;
    const haystack = [item.label, item.group ?? "", ...(item.keywords ?? [])].join(" ").toLowerCase();
    const label = item.label.toLowerCase();
    if (label.startsWith(query)) return -1000;
    if (label.includes(query)) return -500 + label.indexOf(query);
    let at = -1;
    let gaps = 0;
    for (const ch of query) {
        const next = haystack.indexOf(ch, at + 1);
        if (next === -1) return null;
        gaps += next - at - 1;
        at = next;
    }
    return gaps;
}

/** ⌘K jump-to dialog: filter as you type, ↑/↓ to move, Enter to run, Esc to close. */
export function CommandPalette({
    open,
    onOpenChange,
    items,
    placeholder = "Jump to a page or run a command…",
    emptyLabel = "Nothing matches.",
}: CommandPaletteProps) {
    const [query, setQuery] = useState("");
    const [cursor, setCursor] = useState(0);
    const listRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (open) {
            setQuery("");
            setCursor(0);
        }
    }, [open]);

    const results = useMemo(() => {
        const q = query.trim().toLowerCase();
        return items
            .map((item, index) => ({ item, index, rank: score(q, item) }))
            .filter((r): r is { item: CommandPaletteItem; index: number; rank: number } => r.rank !== null)
            .sort((a, b) => (q ? a.rank - b.rank : a.index - b.index))
            .map((r) => r.item);
    }, [items, query]);

    useEffect(() => {
        listRef.current?.querySelector(`[data-index="${cursor}"]`)?.scrollIntoView({ block: "nearest" });
    }, [cursor]);

    const run = (item: CommandPaletteItem | undefined) => {
        if (!item) return;
        onOpenChange(false);
        item.onSelect();
    };

    const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
        if (e.key === "ArrowDown") {
            e.preventDefault();
            setCursor((c) => Math.min(c + 1, results.length - 1));
        } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setCursor((c) => Math.max(c - 1, 0));
        } else if (e.key === "Enter") {
            e.preventDefault();
            run(results[cursor]);
        }
    };

    let lastGroup: string | undefined;

    return (
        <Dialog.Root open={open} onOpenChange={onOpenChange}>
            <Dialog.Portal>
                <Dialog.Backdrop className={styles.backdrop} />
                <Dialog.Popup className={styles.popup} aria-label="Command palette">
                    <div className={styles.inputRow}>
                        <Search size={16} className={styles.inputIcon} />
                        <input
                            className={styles.input}
                            value={query}
                            placeholder={placeholder}
                            onChange={(e) => {
                                setQuery(e.target.value);
                                setCursor(0);
                            }}
                            onKeyDown={onKeyDown}
                            role="combobox"
                            aria-expanded="true"
                            aria-controls="octo-command-list"
                            aria-activedescendant={results[cursor] ? `octo-cmd-${results[cursor].id}` : undefined}
                        />
                        <kbd className={styles.kbd}>esc</kbd>
                    </div>
                    <div className={styles.list} ref={listRef} id="octo-command-list" role="listbox">
                        {results.length === 0 && <div className={styles.empty}>{emptyLabel}</div>}
                        {results.map((item, index) => {
                            const heading = !query && item.group !== lastGroup ? item.group : undefined;
                            lastGroup = item.group;
                            return (
                                <div key={item.id}>
                                    {heading && <div className={styles.group}>{heading}</div>}
                                    <div
                                        id={`octo-cmd-${item.id}`}
                                        className={styles.item}
                                        role="option"
                                        aria-selected={index === cursor}
                                        data-index={index}
                                        data-active={index === cursor ? "true" : undefined}
                                        onMouseMove={() => setCursor(index)}
                                        onClick={() => run(item)}
                                    >
                                        <span className={styles.icon}>{renderIcon(item.icon)}</span>
                                        <span className={styles.label}>{item.label}</span>
                                        {item.hint != null && <span className={styles.hint}>{item.hint}</span>}
                                    </div>
                                </div>
                            );
                        })}
                    </div>
                    <div className={styles.footer}>
                        <span>
                            <kbd className={styles.kbd}>↑</kbd>
                            <kbd className={styles.kbd}>↓</kbd> move
                        </span>
                        <span>
                            <kbd className={styles.kbd}>↵</kbd> open
                        </span>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
