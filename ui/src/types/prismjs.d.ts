// prismjs ships without types and @types/prismjs is not installed; the app only
// re-exposes the core instance as a global (see shared/lib/prism-global.ts).
declare module "prismjs" {
  const Prism: Record<string, unknown>;
  export default Prism;
}
