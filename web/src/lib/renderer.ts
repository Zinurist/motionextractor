// Draws preview frames: WebGL2 when available, Canvas 2D otherwise.

import { FORMAT_JPEG, type FrameMessage } from './protocol';
import type { View } from './types';

/** A destination rectangle in canvas pixels. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Quad {
  rect: Rect;
  /** `output`/`original`, or a split showing `original` left of `split` and `output` right of it. */
  content: View | { split: number };
}

export interface Renderer {
  upload(f: FrameMessage): Promise<void>;
  draw(quads: Quad[], smooth: boolean): void;
  clear(view: View): void;
}

async function decodeJpeg(f: FrameMessage): Promise<ImageBitmap> {
  const bytes = new Uint8Array(f.payload);
  return createImageBitmap(new Blob([bytes], { type: 'image/jpeg' }));
}

const VERTEX = `#version 300 es
in vec2 a_pos;
uniform vec4 u_rect;
out vec2 v_uv;
void main() {
  v_uv = a_pos;
  gl_Position = vec4(mix(u_rect.xy, u_rect.zw, a_pos), 0.0, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_output;
uniform sampler2D u_original;
uniform int u_mode;
uniform float u_split;
out vec4 color;
void main() {
  bool original = u_mode == 1 || (u_mode == 2 && v_uv.x < u_split);
  color = original ? texture(u_original, v_uv) : texture(u_output, v_uv);
}`;

class GlRenderer implements Renderer {
  private textures: Record<View, WebGLTexture>;
  private program: WebGLProgram;
  private u: Record<string, WebGLUniformLocation | null> = {};

  constructor(private gl: WebGL2RenderingContext) {
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? '');
      return s;
    };
    const p = gl.createProgram()!;
    gl.attachShader(p, compile(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(p, compile(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? '');
    this.program = p;
    gl.useProgram(p);
    for (const name of ['u_rect', 'u_output', 'u_original', 'u_mode', 'u_split']) {
      this.u[name] = gl.getUniformLocation(p, name);
    }
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(p, 'a_pos');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const texture = () => {
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGB8,
        1,
        1,
        0,
        gl.RGB,
        gl.UNSIGNED_BYTE,
        new Uint8Array(3),
      );
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return t;
    };
    this.textures = { output: texture(), original: texture() };
  }

  async upload(f: FrameMessage): Promise<void> {
    const gl = this.gl;
    const bitmap = f.format === FORMAT_JPEG ? await decodeJpeg(f) : null;
    gl.bindTexture(gl.TEXTURE_2D, this.textures[f.view]);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    if (bitmap) {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, bitmap);
      bitmap.close();
    } else {
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGB8,
        f.width,
        f.height,
        0,
        gl.RGB,
        gl.UNSIGNED_BYTE,
        f.payload,
      );
    }
  }

  clear(view: View): void {
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.textures[view]);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB8, 1, 1, 0, gl.RGB, gl.UNSIGNED_BYTE, new Uint8Array(3));
  }

  draw(quads: Quad[], smooth: boolean): void {
    const gl = this.gl;
    const { width, height } = gl.canvas;
    gl.viewport(0, 0, width, height);
    gl.clearColor(0.07, 0.075, 0.086, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.useProgram(this.program);
    const filter = smooth ? gl.LINEAR : gl.NEAREST;
    (['output', 'original'] as View[]).forEach((view, unit) => {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(gl.TEXTURE_2D, this.textures[view]);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
    });
    gl.uniform1i(this.u.u_output, 0);
    gl.uniform1i(this.u.u_original, 1);
    for (const q of quads) {
      const { x, y, w, h } = q.rect;
      const cx = (px: number) => (px / width) * 2 - 1;
      const cy = (py: number) => 1 - (py / height) * 2;
      gl.uniform4f(this.u.u_rect, cx(x), cy(y), cx(x + w), cy(y + h));
      if (typeof q.content === 'string') {
        gl.uniform1i(this.u.u_mode, q.content === 'original' ? 1 : 0);
      } else {
        gl.uniform1i(this.u.u_mode, 2);
        gl.uniform1f(this.u.u_split, q.content.split);
      }
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
  }
}

class CanvasRenderer implements Renderer {
  private images: Record<View, HTMLCanvasElement> = {
    output: document.createElement('canvas'),
    original: document.createElement('canvas'),
  };

  constructor(private ctx: CanvasRenderingContext2D) {}

  async upload(f: FrameMessage): Promise<void> {
    const c = this.images[f.view];
    c.width = f.width;
    c.height = f.height;
    const ctx = c.getContext('2d')!;
    if (f.format === FORMAT_JPEG) {
      const bitmap = await decodeJpeg(f);
      ctx.drawImage(bitmap, 0, 0);
      bitmap.close();
      return;
    }
    const img = ctx.createImageData(f.width, f.height);
    for (let i = 0, j = 0; i < f.payload.length; i += 3, j += 4) {
      img.data[j] = f.payload[i];
      img.data[j + 1] = f.payload[i + 1];
      img.data[j + 2] = f.payload[i + 2];
      img.data[j + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
  }

  clear(view: View): void {
    this.images[view].width = 0;
  }

  draw(quads: Quad[], smooth: boolean): void {
    const ctx = this.ctx;
    ctx.fillStyle = '#121316';
    ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    ctx.imageSmoothingEnabled = smooth;
    const blit = (view: View, r: Rect, clip?: Rect) => {
      const img = this.images[view];
      if (!img.width) return;
      ctx.save();
      if (clip) {
        ctx.beginPath();
        ctx.rect(clip.x, clip.y, clip.w, clip.h);
        ctx.clip();
      }
      ctx.drawImage(img, r.x, r.y, r.w, r.h);
      ctx.restore();
    };
    for (const q of quads) {
      if (typeof q.content === 'string') {
        blit(q.content, q.rect);
      } else {
        const sx = q.rect.w * q.content.split;
        blit('original', q.rect, { ...q.rect, w: sx });
        blit('output', q.rect, { ...q.rect, x: q.rect.x + sx, w: q.rect.w - sx });
      }
    }
  }
}

export function createRenderer(canvas: HTMLCanvasElement): Renderer {
  const gl = canvas.getContext('webgl2', { antialias: false, preserveDrawingBuffer: true });
  if (gl) {
    try {
      return new GlRenderer(gl);
    } catch (e) {
      console.warn('WebGL2 renderer failed, using Canvas 2D', e);
    }
  }
  return new CanvasRenderer(canvas.getContext('2d')!);
}
