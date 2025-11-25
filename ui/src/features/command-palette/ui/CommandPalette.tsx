import {
  createSignal,
  createMemo,
  createEffect,
  onMount,
  onCleanup,
  For,
  Show,
  type ParentComponent,
} from "solid-js";
import { useNavigate, useLocation } from "@solidjs/router";
import { useUnit } from "effector-solid";
import { Portal } from "solid-js/web";
import {
  type Command,
  createNavigationCommands,
  actionCommands,
  systemCommands,
  restConsoleCommands,
} from "../model/commands";
import {
  $isCommandPaletteOpen,
  closeCommandPalette,
  toggleCommandPalette,
} from "../model/store";
import styles from "./CommandPalette.module.css";

// Kbd component for keyboard shortcuts
const Kbd = (props: { children: string }) => (
  <kbd class={styles.kbd}>{props.children}</kbd>
);

// Fuzzy match function
const fuzzyMatch = (text: string, query: string): boolean => {
  const normalizedText = text.toLowerCase();
  const normalizedQuery = query.toLowerCase();

  if (normalizedQuery.length === 0) return true;
  if (normalizedText.includes(normalizedQuery)) return true;

  // Simple fuzzy matching
  let queryIndex = 0;
  for (let i = 0; i < normalizedText.length && queryIndex < normalizedQuery.length; i++) {
    if (normalizedText[i] === normalizedQuery[queryIndex]) {
      queryIndex++;
    }
  }
  return queryIndex === normalizedQuery.length;
};

// Command Item component
interface CommandItemProps {
  command: Command;
  active: boolean;
  onSelect: () => void;
}

const CommandItem = (props: CommandItemProps) => {
  const hasShortcut = () => props.command.shortcut && props.command.shortcut.length > 0;

  return (
    <div
      class={`${styles.item} ${props.active ? styles.active : ""}`}
      onClick={props.onSelect}
      onMouseEnter={() => {}}
      data-active={props.active}
    >
      <div class={styles.itemContent}>
        <Show when={props.command.icon}>
          <div class={styles.icon}>{props.command.icon}</div>
        </Show>
        <div class={styles.itemText}>
          <span class={styles.itemName}>{props.command.name}</span>
          <Show when={props.command.subtitle}>
            <span class={styles.itemSubtitle}>{props.command.subtitle}</span>
          </Show>
        </div>
      </div>

      <Show when={hasShortcut()}>
        <div class={styles.shortcuts}>
          <For each={props.command.shortcut}>
            {(key) => <Kbd>{key}</Kbd>}
          </For>
        </div>
      </Show>
    </div>
  );
};

// Group commands by section
interface GroupedCommands {
  section: string;
  commands: Command[];
}

const groupCommands = (commands: Command[]): GroupedCommands[] => {
  const groups = new Map<string, Command[]>();

  for (const command of commands) {
    const section = command.section || "Other";
    const existing = groups.get(section) || [];
    existing.push(command);
    groups.set(section, existing);
  }

  return Array.from(groups.entries()).map(([section, commands]) => ({
    section,
    commands,
  }));
};

// Main Command Palette Modal
const CommandPaletteModal = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const isOpen = useUnit($isCommandPaletteOpen);

  const [query, setQuery] = createSignal("");
  const [activeIndex, setActiveIndex] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;

  // Build commands with navigation
  const allCommands = createMemo(() => {
    const navCommands = createNavigationCommands(navigate);
    return [...navCommands, ...systemCommands, ...actionCommands, ...restConsoleCommands];
  });

  // Filter commands based on current route
  const contextCommands = createMemo(() => {
    return allCommands().filter((command) => {
      if (command.section === "Navigation" || command.section === "System" || command.section === "Theme") {
        return true;
      }
      if (command.section === "REST Console") {
        return location.pathname === "/console";
      }
      return true;
    });
  });

  // Filter commands based on search query
  const filteredCommands = createMemo(() => {
    const q = query();
    if (!q) return contextCommands();

    return contextCommands().filter((command) => {
      const searchText = `${command.name} ${command.keywords || ""} ${command.subtitle || ""}`;
      return fuzzyMatch(searchText, q);
    });
  });

  // Group filtered commands
  const groupedCommands = createMemo(() => groupCommands(filteredCommands()));

  // Flat list for keyboard navigation
  const flatCommands = createMemo(() => filteredCommands());

  // Reset active index when results change
  createEffect(() => {
    flatCommands();
    setActiveIndex(0);
  });

  // Focus input when opened
  createEffect(() => {
    if (isOpen()) {
      setQuery("");
      setActiveIndex(0);
      setTimeout(() => inputRef?.focus(), 10);
    }
  });

  // Handle keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const commands = flatCommands();

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActiveIndex((i) => Math.min(i + 1, commands.length - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIndex((i) => Math.max(i - 1, 0));
        break;
      case "Enter":
        e.preventDefault();
        const activeCommand = commands[activeIndex()];
        if (activeCommand) {
          executeCommand(activeCommand);
        }
        break;
      case "Escape":
        e.preventDefault();
        closeCommandPalette();
        break;
    }
  };

  const executeCommand = (command: Command) => {
    closeCommandPalette();
    if (command.perform) {
      command.perform();
    }
  };

  return (
    <Show when={isOpen()}>
      <Portal>
        <div class={styles.overlay} onClick={() => closeCommandPalette()}>
          <div class={styles.container} onClick={(e) => e.stopPropagation()}>
            <div class={styles.search}>
              <svg class={styles.searchIcon} viewBox="0 0 20 20" fill="currentColor">
                <path
                  fill-rule="evenodd"
                  d="M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z"
                  clip-rule="evenodd"
                />
              </svg>
              <input
                ref={inputRef}
                type="text"
                class={styles.searchInput}
                placeholder="Search commands..."
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
              />
              <Kbd>esc</Kbd>
            </div>

            <div class={styles.results}>
              <Show
                when={flatCommands().length > 0}
                fallback={
                  <div class={styles.noResults}>No commands found</div>
                }
              >
                <For each={groupedCommands()}>
                  {(group) => (
                    <>
                      <div class={styles.section}>{group.section}</div>
                      <For each={group.commands}>
                        {(command) => {
                          const commandIndex = createMemo(() =>
                            flatCommands().findIndex((c) => c.id === command.id)
                          );
                          return (
                            <CommandItem
                              command={command}
                              active={activeIndex() === commandIndex()}
                              onSelect={() => executeCommand(command)}
                            />
                          );
                        }}
                      </For>
                    </>
                  )}
                </For>
              </Show>
            </div>

            <div class={styles.footer}>
              <span class={styles.footerHint}>
                <Kbd>↑</Kbd>
                <Kbd>↓</Kbd>
                <span>to navigate</span>
              </span>
              <span class={styles.footerHint}>
                <Kbd>↵</Kbd>
                <span>to select</span>
              </span>
              <span class={styles.footerHint}>
                <Kbd>esc</Kbd>
                <span>to close</span>
              </span>
            </div>
          </div>
        </div>
      </Portal>
    </Show>
  );
};

// Provider component with global keyboard listener
export const CommandPaletteProvider: ParentComponent = (props) => {
  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Cmd/Ctrl + K to open
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        toggleCommandPalette();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  return (
    <>
      {props.children}
      <CommandPaletteModal />
    </>
  );
};

// Hook to control the command palette programmatically
export const useCommandPalette = () => {
  const isOpen = useUnit($isCommandPaletteOpen);
  return {
    isOpen,
    open: () => toggleCommandPalette(),
    close: () => closeCommandPalette(),
  };
};

// Trigger component for header button
export const CommandPaletteTrigger: ParentComponent = (props) => {
  return (
    <div onClick={() => toggleCommandPalette()} style={{ cursor: "pointer" }}>
      {props.children}
    </div>
  );
};
