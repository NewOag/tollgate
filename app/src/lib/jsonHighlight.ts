// Hand-rolled JSON syntax highlighting (no library, same "roll our own to
// keep the design system in control of the palette" approach the charts
// already use) — renders into `{@html ...}`, so every piece of raw text
// is HTML-escaped before it's inserted into the result: request/response
// bodies are untrusted content and would otherwise be an XSS vector.

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}

const TOKEN_RE = /("(\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(\.\d+)?([eE][+-]?\d+)?)/g;

function classFor(token: string): string {
  if (token.startsWith('"')) return token.endsWith(":") ? "jt-key" : "jt-string";
  if (token === "true" || token === "false") return "jt-bool";
  if (token === "null") return "jt-null";
  return "jt-number";
}

export function highlightJson(raw: string): string {
  if (!raw) return "";
  let pretty: string;
  try {
    pretty = JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return escapeHtml(raw);
  }

  let result = "";
  let lastIndex = 0;
  for (const match of pretty.matchAll(TOKEN_RE)) {
    const text = match[0];
    const index = match.index ?? 0;
    result += escapeHtml(pretty.slice(lastIndex, index));
    result += `<span class="${classFor(text)}">${escapeHtml(text)}</span>`;
    lastIndex = index + text.length;
  }
  result += escapeHtml(pretty.slice(lastIndex));
  return result;
}
