// `@mdxeditor/editor` pulls in `@lexical/code`, which imports prismjs core
// followed by a set of `prismjs/components/prism-*.js` language files. Those
// language files register grammars via a *bare* global `Prism` reference
// (e.g. `Prism.languages.clike = {...}`). In a classic <script> world prismjs
// core assigns `window.Prism`, but once the whole graph is bundled to an ESM
// chunk by Vite/Rolldown that global assignment no longer reaches the language
// modules, so they throw `ReferenceError: Prism is not defined` at eval time.
//
// This side-effect module runs prismjs core and pins the instance on
// `globalThis` BEFORE the app tree (and therefore lexical's language modules)
// is evaluated. It must be imported ahead of `./app` in the entrypoint.
import Prism from "prismjs";

(globalThis as unknown as { Prism: typeof Prism }).Prism = Prism;
