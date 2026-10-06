# rust-webp-converter

[![npm](https://img.shields.io/npm/v/rust-webp-converter)](https://www.npmjs.com/package/rust-webp-converter)
[![Release](https://github.com/pedromneto97/webp-converter/actions/workflows/release.yaml/badge.svg)](https://github.com/pedromneto97/webp-converter/actions/workflows/release.yaml)

Converts images to WebP in Node.js. It is a native addon written in Rust with [napi-rs](https://napi.rs) and uses
Google's [libwebp](https://chromium.googlesource.com/webm/libwebp).

- Reads JPEG, PNG, GIF, BMP, TIFF, WebP, QOI, ICO, TGA, PNM, DDS, OpenEXR, Radiance HDR and Farbfeld, decoded by the
  Rust [`image`](https://crates.io/crates/image) crate. The format comes from the file's magic bytes, not from its
  extension. AVIF is not supported as input.
- Converts animated GIFs to animated WebP and keeps frame delays and loop count.
- Picks the quality automatically: it encodes at several qualities and keeps the best size/quality ratio. You can also
  set a fixed quality.
- Applies the EXIF orientation, so photos from phones come out upright.
- Runs on the libuv threadpool, so it does not block the event loop.
- Ships prebuilt binaries. No compiler, no `node-gyp` and no install scripts.

## Requirements

- Node.js `>= 22.14.0 < 23` or `>= 23.6.0`.
- One of the supported platforms:

| OS      | CPU   | libc        | Package                                      |
| ------- | ----- | ----------- | -------------------------------------------- |
| macOS   | arm64 | —           | `rust-webp-converter-darwin-arm64`           |
| Linux   | x64   | glibc, musl | `rust-webp-converter-linux-x64-{gnu,musl}`   |
| Linux   | arm64 | glibc, musl | `rust-webp-converter-linux-arm64-{gnu,musl}` |
| Windows | x64   | —           | `rust-webp-converter-win32-x64-msvc`         |

macOS x64 (Intel), Windows arm64 and WebAssembly are not supported. On those platforms the install succeeds, but
importing the package throws `Cannot find native binding`.

## Install

```bash
npm install rust-webp-converter
# or
yarn add rust-webp-converter
pnpm add rust-webp-converter
```

The binary for your platform comes from an optional dependency. Do not install with `--omit=optional` or
`--no-optional`, or the binary is missing. If you install on one platform and run on another (for example
`npm ci` on macOS, then copy `node_modules` into a Linux Docker image), run the install inside the target image
instead.

## Usage

ES modules:

```js
import { readFile, writeFile } from 'node:fs/promises'

import { convert, convertFile } from 'rust-webp-converter'

// Buffer in, Buffer out
const input = await readFile('photo.jpg')
const { data, quality, frameCount } = await convert(input)
await writeFile('photo.webp', data)

// File in, file out
await convertFile('animation.gif', 'animation.webp', { minQuality: 60 })

// Fixed quality, no sweep (faster)
const { data: fast } = await convert(input, { quality: 75 })
```

CommonJS:

```js
const { convert } = require('rust-webp-converter')
```

TypeScript types ship with the package (`index.d.ts`).

### Handling errors

```js
try {
  const { data } = await convert(upload)
} catch (error) {
  if (error.code === 'InvalidArg') {
    // bad options, thrown synchronously by convert()
  } else {
    // decode or encode failure: error.message says what went wrong
  }
}
```

More examples, including an HTTP upload endpoint with input size and concurrency limits, are in
[`examples/`](./examples).

## API

### `convert(input: Buffer, options?: ConvertOptions): Promise<ConvertResult>`

Converts an encoded image to WebP. The input is copied before conversion starts, so you can reuse the buffer right
away.

```ts
interface ConvertResult {
  data: Buffer // WebP file contents
  quality: number // quality the output was encoded at (0-100)
  frameCount: number // frames in the output; 1 for a still image
}
```

### `convertFile(inputPath: string, outputPath: string, options?: ConvertOptions): Promise<ConvertFileResult>`

Reads `inputPath`, converts it, and writes the WebP to `outputPath`. The result is `{ quality, frameCount }`.

`outputPath` is overwritten if it exists. The parent directory must already exist, otherwise the promise rejects.

### `ConvertOptions`

| Option       | Type     | Default | Description                                                           |
| ------------ | -------- | ------- | --------------------------------------------------------------------- |
| `minQuality` | `number` | `80`    | Lowest quality the sweep tries (integer, 0-100).                      |
| `quality`    | `number` | —       | Encode once at this quality (integer, 0-100). Overrides `minQuality`. |

## How it works

**Quality sweep.** The converter encodes the image at several qualities from `minQuality` to 100. It keeps the output
with the lowest `bytes / quality` ratio. Still images try every second quality level (11 encodes at the default).
Animations try at most 4 levels, because each level re-encodes every frame. Set `quality` to encode once.

**Animated GIFs.** The output is an animated WebP with the same frame delays and loop count. Delays have a 10 ms floor.
libwebp merges identical consecutive frames, so `frameCount` can be lower than the GIF's frame count. If only one frame
remains, the output is a still WebP and `frameCount` is `1`. A single-frame GIF is encoded as a still image.

**Color types.** Grayscale, 16-bit and float images are converted to 8-bit RGB, or to RGBA when they have an alpha
channel.

**Limits.** WebP supports at most 16383 pixels on each axis. Larger images are rejected after reading only the image
header. Decoders run with the `image` crate's 512 MiB allocation limit.

## Limitations

- Output is always lossy WebP (animations may mix lossy and lossless frames). There is no lossless mode.
- Metadata is not copied: EXIF, XMP and ICC color profiles are dropped. Wide-gamut images (for example Display P3)
  can look less saturated.
- No resizing, cropping or other transforms. Use a full image library such as [sharp](https://sharp.pixelplumbing.com)
  if you need them.
- Only GIF animations are kept. Animated PNG or WebP input becomes a still image.

## Security

If you convert untrusted input, such as user uploads, read the [threat model in SECURITY.md](./SECURITY.md#threat-model-and-known-limitations).
In short: limit the input size, limit concurrent conversions, and never pass user-controlled paths to `convertFile`.

Report vulnerabilities privately, as described in [SECURITY.md](./SECURITY.md).

## Development

Requires a Rust toolchain and a C compiler (libwebp is compiled from source).

```bash
yarn install
yarn build      # release addon
yarn test       # ava tests (build first)
cargo test      # Rust unit tests
yarn bench      # benchmarks
```

Contributor and AI-agent guidance is in [AGENTS.md](./AGENTS.md).

## License

MIT
