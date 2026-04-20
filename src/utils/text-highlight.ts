/**
 * 把 `text` 中的 `keyword` 出现处包上 <mark>, 返回可直接 v-html 的 HTML 字符串.
 * 其他部分做基本 HTML 转义 (& < > " ') 避免 XSS.
 */
export function highlightKeyword(text: string, keyword: string, matchCase = false): string {
  if (!keyword) return escapeHtml(text);
  const hay = matchCase ? text : text.toLowerCase();
  const needle = matchCase ? keyword : keyword.toLowerCase();
  if (needle.length === 0) return escapeHtml(text);

  const parts: string[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    const idx = hay.indexOf(needle, cursor);
    if (idx < 0) {
      parts.push(escapeHtml(text.slice(cursor)));
      break;
    }
    if (idx > cursor) parts.push(escapeHtml(text.slice(cursor, idx)));
    parts.push(`<mark class="kw-hit">${escapeHtml(text.slice(idx, idx + needle.length))}</mark>`);
    cursor = idx + needle.length;
  }
  return parts.join("");
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
