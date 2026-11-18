// Literal queries only. Limits apply to returned text, independently of JSON metadata.
const MAX_EXCERPT_BYTES = 8192;

function clip(text, maximum, fromEnd = false) {
  const bytes = Buffer.from(text);
  let start = fromEnd ? Math.max(0, bytes.length - maximum) : 0;
  let end = fromEnd ? bytes.length : Math.min(bytes.length, maximum);
  while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) start++;
  while (end < bytes.length && (bytes[end] & 0xc0) === 0x80) end--;
  return bytes.subarray(start, end).toString();
}

export function findText(text, queries) {
  const content = text.replace(/\s+/gu, ' ');
  const budget = Math.floor(MAX_EXCERPT_BYTES / queries.length);
  return queries.map(query => {
    const escaped = query.split(/\s+/u).map(part => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('\\s+');
    const expression = new RegExp(escaped, 'giu');
    const result = { query, found: false, excerpts: [], truncated: false };
    let remaining = budget;
    let previousEnd = 0;
    for (const match of content.matchAll(expression)) {
      result.found = true;
      if (match.index + match[0].length <= previousEnd) continue;
      const matchedBytes = Buffer.byteLength(match[0]);
      if (result.excerpts.length === 3 || remaining < matchedBytes + 6) {
        result.truncated = true;
        break;
      }
      const available = Math.min(768, remaining) - matchedBytes - 6;
      const before = clip(content.slice(0, match.index), Math.floor(available / 2), true);
      const after = clip(content.slice(match.index + match[0].length), available - Buffer.byteLength(before));
      const start = match.index - before.length;
      previousEnd = match.index + match[0].length + after.length;
      const excerpt = `${start ? '…' : ''}${before}${match[0]}${after}${previousEnd < content.length ? '…' : ''}`;
      result.excerpts.push(excerpt);
      remaining -= Buffer.byteLength(excerpt);
    }
    return result;
  });
}
