import type { CommandPaletteItem } from "@octofhir/ui-kit";
import { createEvent, createStore } from "effector";
import { useUnit } from "effector-react";
import { useEffect } from "react";

/** Whether the global ⌘K palette is open. */
export const paletteOpenChanged = createEvent<boolean>();
export const $paletteOpen = createStore(false).on(paletteOpenChanged, (_, open) => open);

interface PaletteContribution {
  source: string;
  items: CommandPaletteItem[];
}

const paletteItemsProvided = createEvent<PaletteContribution>();
const paletteItemsWithdrawn = createEvent<string>();

/** Page-provided commands keyed by source, merged into the global palette while the page is mounted. */
export const $paletteContributions = createStore<Record<string, CommandPaletteItem[]>>({})
  .on(paletteItemsProvided, (state, { source, items }) => ({ ...state, [source]: items }))
  .on(paletteItemsWithdrawn, (state, source) => {
    const { [source]: _removed, ...rest } = state;
    return rest;
  });

/** Adds `items` to the global palette for as long as the calling component is mounted. */
export function usePaletteContribution(source: string, items: CommandPaletteItem[]) {
  const [provide, withdraw] = useUnit([paletteItemsProvided, paletteItemsWithdrawn]);
  useEffect(() => {
    provide({ source, items });
  }, [provide, source, items]);
  useEffect(
    () => () => {
      withdraw(source);
    },
    [withdraw, source]
  );
}
