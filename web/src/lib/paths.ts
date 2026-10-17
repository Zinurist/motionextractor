// Path helpers that work for both POSIX and Windows paths coming from the backend.

export function separator(path: string): string {
  return path.includes('\\') && !path.includes('/') ? '\\' : '/';
}

export function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

export function dirname(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i <= 0 ? path.slice(0, i + 1) : path.slice(0, i);
}

export function join(dir: string, name: string): string {
  if (!dir) return name;
  const sep = separator(dir);
  return dir.endsWith('/') || dir.endsWith('\\') ? dir + name : dir + sep + name;
}

function stem(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(0, i) : name;
}

/** Replaces (or adds) the extension; an empty extension removes it. */
export function withExtension(path: string, ext: string): string {
  const dir = dirname(path);
  const name = stem(basename(path)) + (ext ? `.${ext}` : '');
  return dir ? join(dir, name) : name;
}

/** `/videos/clip.mov` → `/videos/clip_motion.mp4`. */
export function defaultOutput(input: string, ext: string): string {
  const name = `${stem(basename(input))}_motion${ext ? `.${ext}` : ''}`;
  const dir = dirname(input);
  return dir ? join(dir, name) : name;
}
