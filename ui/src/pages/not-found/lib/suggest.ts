export interface PageRef {
  label: string;
  section: string;
  path: string;
  aliases: string[];
}

const RESOURCE_TYPE = /^[A-Z][A-Za-z]{2,63}$/;
const RESOURCE_ID = /^[A-Za-z0-9\-.]{1,64}$/;

function segments(path: string): string[] {
  return path.split("/").filter(Boolean).map(decodeURIComponent);
}

/**
 * A FHIR REST address typed into the console (`/Patient/123`, `/fhir/Patient/123/_history/2`)
 * points at the resource browser; anything else gives null.
 */
export function resourceAddress(path: string): string | null {
  const parts = segments(path);
  if (parts[0] === "fhir") parts.shift();
  const [type, id, history] = parts;
  if (!type || !RESOURCE_TYPE.test(type)) return null;
  if (parts.length > 2 && history !== "_history") return null;
  if (id === undefined) return `/resources/${type}`;
  return RESOURCE_ID.test(id) ? `/resources/${type}/${id}` : null;
}

function distance(a: string, b: string): number {
  const row = Array.from({ length: b.length + 1 }, (_, index) => index);
  for (let i = 1; i <= a.length; i += 1) {
    let diagonal = row[0];
    row[0] = i;
    for (let j = 1; j <= b.length; j += 1) {
      const above = row[j];
      row[j] = Math.min(row[j] + 1, row[j - 1] + 1, diagonal + (a[i - 1] === b[j - 1] ? 0 : 1));
      diagonal = above;
    }
  }
  return row[b.length];
}

/**
 * Sidebar pages whose address or name is close to the one that wasn't found: a typo
 * (`/subscription`), an old alias (`/users`), a section prefix (`/auth`) or a deeper path
 * under a page that exists. Best match first.
 */
export function closestPages(path: string, pages: PageRef[], limit = 3): PageRef[] {
  const typed = segments(path.toLowerCase());
  if (typed.length === 0) return [];
  const last = typed[typed.length - 1];
  const joined = `/${typed.join("/")}`;

  const scored = pages.flatMap((page) => {
    const addresses = [page.path, ...page.aliases];
    if (addresses.some((address) => joined.startsWith(`${address}/`) || joined === address)) {
      return [{ page, score: 0 }];
    }
    if (addresses.some((address) => address.startsWith(`${joined}/`))) return [{ page, score: 1 }];
    const names = [page.label.toLowerCase(), ...addresses.flatMap((address) => segments(address).slice(-1))];
    const best = Math.min(...names.map((name) => distance(last, name)));
    const allowed = Math.max(1, Math.floor(last.length / 3));
    return best <= allowed ? [{ page, score: 1 + best }] : [];
  });

  return scored
    .sort((a, b) => a.score - b.score || a.page.label.localeCompare(b.page.label))
    .slice(0, limit)
    .map((entry) => entry.page);
}
