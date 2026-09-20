import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import type { Extension } from "@codemirror/state";

type Factory = () => Extension;

const LANGUAGES: Record<string, Factory> = {
  js: javascript,
  jsx: () => javascript({ jsx: true }),
  ts: () => javascript({ typescript: true }),
  tsx: () => javascript({ jsx: true, typescript: true }),
  mjs: javascript,
  cjs: javascript,
  json: json,
  jsonc: () => json(),
  md: markdown,
  markdown: markdown,
  mdx: markdown,
  py: python,
  pyw: python,
  rs: rust,
  css: css,
  scss: css,
  less: css,
  html: html,
  htm: html,
  vue: html,
  svelte: html,
  xml: html,
  toml: () => json(),
  yaml: () => json(),
  yml: () => json(),
};

export function languageFor(path: string): Extension | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const factory = LANGUAGES[ext];
  return factory ? factory() : null;
}
