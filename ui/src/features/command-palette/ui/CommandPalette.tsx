import { Box, Group, Kbd, Text } from "@mantine/core";
import { useUnit } from "effector-react";
import {
  type ActionImpl,
  KBarAnimator,
  KBarPortal,
  KBarPositioner,
  KBarProvider,
  KBarResults,
  KBarSearch,
  useMatches,
} from "kbar";
import type React from "react";
import { useEffect, useMemo } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { commandToAction, createNavigationCommands, actionCommands, systemCommands, restConsoleCommands } from "../model/commands";
import {
  $isCommandPaletteOpen,
  attachGlobalKeydownListener,
  closeCommandPalette,
  detachGlobalKeydownListener,
  openCommandPalette,
} from "../model/store";
import styles from "./CommandPalette.module.css";

interface RenderResultsProps {
  onRender: (action: ActionImpl, active: boolean) => React.ReactNode;
}

const RenderResults: React.FC<RenderResultsProps> = ({ onRender }) => {
  const { results } = useMatches();

  return (
    <KBarResults
      items={results}
      onRender={({ item, active }) => {
        if (typeof item === "string") {
          // Section header
          return (
            <div className={styles.section}>
              <Text size="xs" fw={600} c="dimmed" tt="uppercase">
                {item}
              </Text>
            </div>
          );
        }

        return onRender(item, active) as React.ReactElement;
      }}
    />
  );
};

interface CommandItemProps {
  action: ActionImpl;
  active: boolean;
}

const CommandItem: React.FC<CommandItemProps> = ({ action, active }) => {
  const hasShortcut = action.shortcut && action.shortcut.length > 0;

  return (
    <Box className={`${styles.item} ${active ? styles.active : ""}`} data-active={active}>
      <Group justify="space-between" wrap="nowrap" w="100%">
        <Group gap="sm" wrap="nowrap">
          {action.icon && <Box className={styles.icon}>{action.icon}</Box>}
          <Box>
            <Text size="sm" fw={500}>
              {action.name}
            </Text>
            {action.subtitle && (
              <Text size="xs" c="dimmed">
                {action.subtitle}
              </Text>
            )}
          </Box>
        </Group>

        {hasShortcut && (
          <Group gap="xs">
            {action.shortcut!.map((key) => (
              <Kbd key={key} size="xs">
                {key}
              </Kbd>
            ))}
          </Group>
        )}
      </Group>
    </Box>
  );
};

interface CommandPaletteProviderProps {
  children: React.ReactNode;
}

export const CommandPaletteProvider: React.FC<CommandPaletteProviderProps> = ({ children }) => {
  const navigate = useNavigate();
  const location = useLocation();

  // Generate commands with proper navigation
  const commands = useMemo(() => {
    const navigationCommands = createNavigationCommands(navigate);
    const allCommands = [...navigationCommands, ...systemCommands, ...actionCommands, ...restConsoleCommands];

    // Filter commands based on current route/context
    return allCommands
      .filter((command) => {
        // Always show navigation and system commands
        if (command.section === "Navigation" || command.section === "System") {
          return true;
        }

        // Show REST console commands only when in console
        if (command.section === "REST Console") {
          return location.pathname === "/console";
        }

        // Show theme commands everywhere
        if (command.section === "Theme") {
          return true;
        }

        return true;
      })
      .sort((a, b) => (b.priority || 0) - (a.priority || 0));
  }, [navigate, location.pathname]);

  // Convert commands to kbar actions
  const actions = useMemo(() => commands.map(commandToAction), [commands]);

  useEffect(() => {
    attachGlobalKeydownListener();
    return detachGlobalKeydownListener;
  }, []);

  return (
    <KBarProvider
      actions={actions}
      options={{
        enableHistory: true,
        disableScrollbarManagement: true,
      }}
    >
      {children}

      <KBarPortal>
        <KBarPositioner className={styles.positioner}>
          <KBarAnimator className={styles.animator}>
            <Box className={styles.search}>
              <KBarSearch
                className={styles.searchInput}
                placeholder="Search commands..."
                defaultPlaceholder="Search commands..."
              />
            </Box>

            <Box className={styles.results}>
              <RenderResults
                onRender={(action, active) => <CommandItem action={action} active={active} />}
              />
            </Box>
          </KBarAnimator>
        </KBarPositioner>
      </KBarPortal>
    </KBarProvider>
  );
};

// Hook to control the command palette programmatically
export const useCommandPalette = () => {
  return {
    open: openCommandPalette,
    close: closeCommandPalette,
    isOpen: useUnit($isCommandPaletteOpen),
  };
};

// Component to trigger command palette (for header button)
export const CommandPaletteTrigger: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  return (
    <Box onClick={() => openCommandPalette()} style={{ cursor: "pointer" }}>
      {children}
    </Box>
  );
};
