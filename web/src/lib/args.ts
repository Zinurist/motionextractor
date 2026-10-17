/** Splits a command line into arguments, honouring single and double quotes. */
export function splitArgs(s: string): string[] {
  const out: string[] = [];
  let cur = '';
  let quote: string | null = null;
  let has = false;
  for (const ch of s) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      has = true;
    } else if (/\s/.test(ch)) {
      if (has || cur) out.push(cur);
      cur = '';
      has = false;
    } else {
      cur += ch;
    }
  }
  if (has || cur) out.push(cur);
  return out;
}

/** Inverse of `splitArgs` for display. */
export function joinArgs(args: string[]): string {
  return args
    .map((a) => (a === '' || /[\s'"]/.test(a) ? `"${a.replace(/"/g, "'")}"` : a))
    .join(' ');
}
