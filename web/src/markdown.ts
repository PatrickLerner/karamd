// Minimal markdown -> HTML renderer. Supports headings, paragraphs,
// bold/italic/inline code, fenced code blocks, lists, checkboxes and links.
// All source text is HTML-escaped before formatting is applied.

// taskmd/Obsidian wiki links are shown as plain text, not links: `[[Klaus]]`
// becomes `Klaus`, and `[[target|Alias]]` becomes `Alias`.
export function stripWikiLinks(s: string): string {
  return s.replace(
    /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g,
    (_m, target: string, alias?: string) => alias ?? target,
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const SAFE_HREF = /^(https?:\/\/|mailto:|\/|#|\.)/i;

function link(href: string, text: string): string {
  return `<a href="${href}" target="_blank" rel="noopener noreferrer">${text}</a>`;
}

function inline(s: string): string {
  let out = escapeHtml(s);
  out = out.replace(/`([^`]+)`/g, "<code>$1</code>");
  // Markdown links: [text](href).
  out = out.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (match, text, href) =>
    SAFE_HREF.test(href) ? link(href, text) : match,
  );
  // Bare URLs -> autolinks. The prefix guard (start, whitespace, or `(`) keeps
  // this from matching inside an attribute we just wrote (`href="http..."`, a
  // URL is preceded by `"`) or inside `<code>`/`</tag>` (preceded by `>`).
  // Trailing sentence punctuation is left outside the link.
  out = out.replace(
    /(^|[\s(])(https?:\/\/[^\s<)]+)/g,
    (_m, pre: string, url: string) => {
      const trail = url.match(/[.,;:!?]+$/)?.[0] ?? "";
      const clean = trail ? url.slice(0, -trail.length) : url;
      return `${pre}${link(clean, clean)}${trail}`;
    },
  );
  out = out.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  out = out.replace(/\*([^*]+)\*/g, "<em>$1</em>");
  return out;
}

const LIST_ITEM = /^\s*(?:[-*]|\d+\.)\s+/;
const BLOCK_START = /^(#{1,6}\s|```|\s*(?:[-*]|\d+\.)\s)/;

export function renderMarkdown(src: string): string {
  const lines = stripWikiLinks(src).replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim() === "") {
      i++;
      continue;
    }
    if (line.startsWith("```")) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) {
        code.push(lines[i]);
        i++;
      }
      i++; // closing fence (or EOF)
      out.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = heading[1].length;
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      i++;
      continue;
    }
    if (LIST_ITEM.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const items: string[] = [];
      while (
        i < lines.length &&
        LIST_ITEM.test(lines[i]) &&
        /^\s*\d+\./.test(lines[i]) === ordered
      ) {
        const item = lines[i].replace(LIST_ITEM, "");
        const check = item.match(/^\[([ xX])\]\s+(.*)$/);
        if (check) {
          const checked = check[1].toLowerCase() === "x" ? " checked" : "";
          items.push(
            `<li class="task-check"><input type="checkbox" disabled${checked}> ${inline(check[2])}</li>`,
          );
        } else {
          items.push(`<li>${inline(item)}</li>`);
        }
        i++;
      }
      const tag = ordered ? "ol" : "ul";
      out.push(`<${tag}>${items.join("")}</${tag}>`);
      continue;
    }
    const para: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !BLOCK_START.test(lines[i])
    ) {
      para.push(lines[i]);
      i++;
    }
    out.push(`<p>${inline(para.join(" "))}</p>`);
  }
  return out.join("\n");
}
