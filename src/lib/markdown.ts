import MarkdownIt from "markdown-it";

// html:false escapes raw HTML, so rendered chat output is safe to inject.
const md = new MarkdownIt({ html: false, linkify: true, breaks: true });

export function renderMarkdown(text: string): string {
  return md.render(text);
}
