type Json = Record<string, unknown>;

function isObject(v: unknown): v is Json {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** Deep merge of `patch` into `target` (returns a new object; arrays are replaced). */
export function deepMerge<T>(target: T, patch: unknown): T {
  if (!isObject(target) || !isObject(patch)) return patch as T;
  const out: Json = { ...target };
  for (const [k, v] of Object.entries(patch)) {
    out[k] = isObject(v) && isObject(out[k]) ? deepMerge(out[k], v) : v;
  }
  return out as T;
}
