# rust-webp-converter

Converts images to WebP in Node.js. It is a native addon written in Rust with [napi-rs](https://napi.rs) and uses
Google's [libwebp](https://chromium.googlesource.com/webm/libwebp).

- Reads JPEG, PNG, GIF, BMP, TIFF, WebP, QOI, ICO, TGA, PNM, DDS, OpenEXR, Radiance HDR and Farbfeld, decoded by the
  Rust [`image`](https://crates.io/crates/image) crate. The format comes from the file's magic bytes, not from its
  extension. AVIF is not supported as input.
- Converts animated GIFs to animated WebP and keeps frame delays and loop count.
- Picks the quality automatically: it encodes at several qualities and keeps the best size/quality ratio. You can also
  set a fixed quality.
- Runs on the libuv threadpool, so it does not block the event loop.

Prebuilt binaries: macOS arm64, Linux x64/arm64 (glibc and musl), Windows x64. Node.js `>= 22.14`. The list matches
`napi.targets` in `package.json`.

## Install

```bash
npm install rust-webp-converter
```

## Usage

```ts
import { readFile, writeFile } from 'node:fs/promises'

import { convert, convertFile } from 'rust-webp-converter'

// Buffer in, Buffer out
const { data, quality, frameCount } = await convert(await readFile('photo.jpg'))
await writeFile('photo.webp', data)

// File in, file out
await convertFile('animation.gif', 'animation.webp', { minQuality: 60 })

// Fixed quality, no sweep
await convert(input, { quality: 75 })
```

## API

### `convert(input: Buffer, options?: ConvertOptions): Promise<ConvertResult>`

Converts an encoded image to WebP.

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
with the lowest `bytes / quality` ratio. Still images try every second quality level. Animations try at most 4 levels,
because each level re-encodes every frame.

**Animated GIFs.** The output is an animated WebP with the same frame delays and loop count. Delays have a 10 ms floor.
libwebp merges identical consecutive frames, so `frameCount` can be lower than the GIF's frame count. If only one frame
remains, the output is a still WebP and `frameCount` is `1`. A single-frame GIF is encoded as a still image.

**Color types.** Grayscale, 16-bit and float images are converted to 8-bit RGB, or to RGBA when they have an alpha
channel.

**Limits.** WebP supports at most 16383 pixels on each axis. Larger images are rejected.

## Errors

- Invalid options make `convert` and `convertFile` throw synchronously, with `code` set to `InvalidArg`.
- Conversion failures reject the promise with a descriptive message, for example
  `Unsupported or unrecognized image format`, `Failed to decode image: …`, `Input file not found: …`,
  `Failed to write output file: …` or `Image dimensions 20000x100 are outside the WebP limit of 16383x16383`.

## Development

Requires a Rust toolchain and a C compiler (libwebp is compiled from source).

```bash
yarn install
yarn build      # release addon
yarn test       # ava tests (build first)
cargo test      # Rust unit tests
yarn bench      # benchmarks
```

## License

MIT
