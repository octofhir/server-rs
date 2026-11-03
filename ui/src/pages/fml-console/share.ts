/**
 * Shareable links: the map, the input and the models packed into the URL fragment
 * (`#s=<deflate, base64url>`), so nothing is stored on the server and the link works for anyone
 * who can open the console.
 */

export interface SharedState {
  source: string;
  input: string;
  models?: string;
}

const PREFIX = "#s=";

function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const base64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

async function pipe(
  bytes: Uint8Array<ArrayBuffer>,
  stream: CompressionStream | DecompressionStream
): Promise<Uint8Array<ArrayBuffer>> {
  const out = new Blob([bytes]).stream().pipeThrough(stream);
  return new Uint8Array(await new Response(out).arrayBuffer());
}

/** A link to this page that opens with `state`. */
export async function shareLink(state: SharedState): Promise<string> {
  const json = new TextEncoder().encode(JSON.stringify(state));
  const packed = await pipe(json, new CompressionStream("deflate-raw"));
  const url = new URL(window.location.href);
  url.hash = PREFIX.slice(1) + toBase64Url(packed);
  return url.toString();
}

/** The state a shared link carries, if the current URL is one. */
export async function sharedState(hash: string): Promise<SharedState | undefined> {
  if (!hash.startsWith(PREFIX)) return undefined;
  try {
    const bytes = await pipe(fromBase64Url(hash.slice(PREFIX.length)), new DecompressionStream("deflate-raw"));
    const state = JSON.parse(new TextDecoder().decode(bytes)) as SharedState;
    return typeof state.source === "string" && typeof state.input === "string" ? state : undefined;
  } catch {
    return undefined;
  }
}
