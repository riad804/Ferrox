# ferrox-web

WebAssembly bindings for the [ferrox](../README.md) editing SDK — the same `Editor`
API as the Kotlin/Swift (UniFFI) bindings, for the browser.

## Build

```sh
cargo install wasm-pack        # once
./scripts/build-web.sh         # → web/pkg/ (wasm + JS + .d.ts + package.json)
```

`build-web.sh bundler` (default) targets webpack/vite; pass `web` for native ESM or
`nodejs` for Node.

## Usage

```ts
import init, { Editor } from "ferrox-web";

await init();                                   // load the wasm module

const editor = new Editor(1920, 1080, 30);      // width, height, fps
const track = editor.addTrack();

editor.addClipJson(track, JSON.stringify({
  source: { kind: "solid", width: 1920, height: 1080, r: 20, g: 30, b: 40, a: 255 },
  start: 0, duration: 5,
}));

// Compose a frame → RGBA bytes → <canvas>
const rgba = editor.renderFrame(1.0, 1920, 1080);        // Uint8Array
const img = new ImageData(new Uint8ClampedArray(rgba), 1920, 1080);
canvas.getContext("2d").putImageData(img, 0, 0);

editor.setBlendMode(track, 0, "screen");
editor.addKeyframe(track, 0, "opacity", 0.0, 0.0);
editor.addKeyframe(track, 0, "opacity", 1.0, 1.0);

editor.undo();                                  // full undo/redo history
const json = editor.saveJson();                 // persist the project
```

## API (generated `.d.ts`)

`wasm-pack` emits `ferrox_web.d.ts` typing the `Editor` class:

```ts
export class Editor {
  constructor(width: number, height: number, fps: number);
  static fromJson(json: string): Editor;
  addTrack(): number;
  addClipJson(track: number, clip_json: string): void;
  removeClip(track: number, index: number): void;
  moveClip(track: number, index: number, new_start: number): void;
  trimClip(track: number, index: number, start: number, duration: number): void;
  setBlendMode(track: number, index: number, mode: string): void;
  setColorGradeJson(track: number, index: number, grade_json: string): void;
  addKeyframe(track: number, index: number, field: string, t: number, value: number): void;
  renderFrame(t: number, width: number, height: number): Uint8Array;
  undo(): boolean;
  redo(): boolean;
  undoDepth(): number;
  redoDepth(): number;
  saveJson(): string;
  loadJson(json: string): void;
}
```

Projects are the same JSON on every platform — save on web, open on Android/iOS, and vice versa.
