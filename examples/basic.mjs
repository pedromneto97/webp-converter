// Converts a file with each API.
//
//   npm install rust-webp-converter
//   node basic.mjs photo.jpg
import { readFile, writeFile } from 'node:fs/promises'
import { basename, extname } from 'node:path'

import { convert, convertFile } from 'rust-webp-converter'

const inputPath = process.argv[2]
if (!inputPath) {
  console.error('Usage: node basic.mjs <image>')
  process.exit(1)
}
const stem = basename(inputPath, extname(inputPath))

// 1. Buffer in, Buffer out, with the default quality sweep (80-100).
const input = await readFile(inputPath)
const swept = await convert(input)
await writeFile(`${stem}.sweep.webp`, swept.data)
console.log(`sweep: quality ${swept.quality}, ${swept.frameCount} frame(s), ${swept.data.length} bytes`)

// 2. One encode at a fixed quality. Faster than the sweep.
const fixed = await convert(input, { quality: 75 })
await writeFile(`${stem}.q75.webp`, fixed.data)
console.log(`fixed: quality ${fixed.quality}, ${fixed.data.length} bytes`)

// 3. File in, file out. The output directory must exist.
const result = await convertFile(inputPath, `${stem}.file.webp`, { minQuality: 60 })
console.log(`file:  quality ${result.quality}, ${result.frameCount} frame(s)`)

// 4. Errors. Invalid options throw synchronously with code 'InvalidArg'; decode failures reject.
try {
  await convert(Buffer.from('not an image'))
} catch (error) {
  console.log(`error: ${error.message}`)
}
