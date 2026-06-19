import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";

// One frozen renderer: raw HTML disabled (markdown-it escapes it) and links
// linkified. markdown-it's default validateLink already rejects javascript:,
// vbscript:, file: and non-image data: URLs.
const md = new MarkdownIt({ html: false, linkify: true });

// Harden any anchors that survive: external-safe rel + target.
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("rel", "noopener noreferrer nofollow");
    node.setAttribute("target", "_blank");
  }
});

/**
 * Render markdown to sanitized HTML. This is the ONLY place markdown becomes
 * HTML and the only `v-html` sink in the app: two independent layers
 * (markdown-it html:false + DOMPurify) guard it.
 */
export function renderMarkdown(src: string): string {
  return DOMPurify.sanitize(md.render(src));
}
