// Binary frame messages from the WebSocket (see CLAUDE.md §3.4).

export const FRAME_MAGIC = 0x3146584d; // "MXF1" little endian
export const HEADER_LEN = 32;
export const FORMAT_RGB = 0;
export const FORMAT_JPEG = 1;
export const FLAG_EXACT = 1;
export const FLAG_ORIGINAL = 2;

export interface FrameMessage {
  seq: number;
  index: number;
  width: number;
  height: number;
  format: number;
  exact: boolean;
  view: 'output' | 'original';
  rev: number;
  payload: Uint8Array;
}

export function parseFrame(buf: ArrayBuffer): FrameMessage | null {
  if (buf.byteLength < HEADER_LEN) return null;
  const v = new DataView(buf);
  const word = (i: number) => v.getUint32(i * 4, true);
  if (word(0) !== FRAME_MAGIC) return null;
  const flags = word(6);
  return {
    seq: word(1),
    index: word(2),
    width: word(3),
    height: word(4),
    format: word(5),
    exact: (flags & FLAG_EXACT) !== 0,
    view: flags & FLAG_ORIGINAL ? 'original' : 'output',
    rev: word(7),
    payload: new Uint8Array(buf, HEADER_LEN),
  };
}
