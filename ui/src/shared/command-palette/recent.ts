import { createEvent, createStore } from "effector";

const STORAGE_KEY = "octofhir-recent";
const LIMIT = 8;

/** A place the user opened, shown at the top of an empty ⌘K palette. */
export interface RecentPlace {
  path: string;
  label: string;
}

function load(): RecentPlace[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter(
          (entry): entry is RecentPlace =>
            typeof entry?.path === "string" && typeof entry?.label === "string"
        )
      : [];
  } catch {
    return [];
  }
}

export const placeVisited = createEvent<RecentPlace>();

/** Most recent first, one entry per path. */
export const $recentPlaces = createStore<RecentPlace[]>(load()).on(placeVisited, (places, place) =>
  [place, ...places.filter((entry) => entry.path !== place.path)].slice(0, LIMIT)
);

$recentPlaces.watch((places) => {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(places));
  } catch {
    // Storage may be unavailable (private mode); recents are a convenience only.
  }
});
