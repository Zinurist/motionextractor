import { parseFrame, type FrameMessage } from './protocol';
import type { ServerEvent, View } from './types';

/** WebSocket connection with automatic reconnect. */
export class Socket {
  private ws: WebSocket | null = null;
  private seq: Record<View, number> = { output: 0, original: 0 };
  private pending: Record<View, number | null> = { output: null, original: null };
  private retryMs = 250;
  private closed = false;

  onEvent: (e: ServerEvent) => void = () => {};
  onFrame: (f: FrameMessage) => void = () => {};
  onStatus: (connected: boolean) => void = () => {};

  connect(): void {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/api/ws`);
    ws.binaryType = 'arraybuffer';
    ws.onopen = () => {
      this.retryMs = 250;
      this.onStatus(true);
      for (const view of ['output', 'original'] as View[]) {
        const frame = this.pending[view];
        if (frame !== null) this.send(view, frame);
      }
    };
    ws.onmessage = (msg) => {
      if (typeof msg.data === 'string') {
        this.onEvent(JSON.parse(msg.data) as ServerEvent);
      } else {
        const frame = parseFrame(msg.data as ArrayBuffer);
        if (frame) this.onFrame(frame);
      }
    };
    ws.onclose = () => {
      this.onStatus(false);
      this.ws = null;
      if (!this.closed) {
        setTimeout(() => this.connect(), this.retryMs);
        this.retryMs = Math.min(this.retryMs * 2, 4000);
      }
    };
    this.ws = ws;
  }

  close(): void {
    this.closed = true;
    this.ws?.close();
  }

  /** Requests frame `index` of `view`; returns the request's sequence number. */
  request(view: View, index: number): number {
    this.pending[view] = index;
    return this.send(view, index);
  }

  private send(view: View, index: number): number {
    const seq = ++this.seq[view];
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: 'view', seq, frame: index, view }));
      this.pending[view] = null;
    }
    return seq;
  }
}
