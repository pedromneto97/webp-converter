import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import test from 'ava'

import { convert, convertFile } from '../index'

const fixturesDir = join(dirname(fileURLToPath(import.meta.url)), 'fixtures')
const fixture = (name: string) => readFileSync(join(fixturesDir, name))

/** Returns the four-character IDs of the top-level RIFF chunks in a WebP file. */
function chunkIds(webp: Buffer): string[] {
  const ids: string[] = []
  let cursor = 12 // 'RIFF' + u32 size + 'WEBP'

  while (cursor + 8 <= webp.length) {
    const size = webp.readUInt32LE(cursor + 4)
    ids.push(webp.toString('ascii', cursor, cursor + 4))
    // Chunk payloads are padded to an even length.
    cursor += 8 + size + (size & 1)
  }

  return ids
}

function assertIsWebp(t: { is: (actual: unknown, expected: unknown) => void }, data: Buffer) {
  t.is(data.toString('ascii', 0, 4), 'RIFF')
  t.is(data.toString('ascii', 8, 12), 'WEBP')
}

for (const name of ['rgb.jpg', 'rgba.png', 'gray.png']) {
  test(`converts ${name} to a still WebP`, async (t) => {
    const result = await convert(fixture(name))

    assertIsWebp(t, result.data)
    t.is(result.frameCount, 1)
    t.true(result.quality >= 80 && result.quality <= 100)
    t.false(chunkIds(result.data).includes('ANIM'))
  })
}

test('converts an animated GIF to an animated WebP', async (t) => {
  const result = await convert(fixture('animated.gif'))

  assertIsWebp(t, result.data)
  t.is(result.frameCount, 3)

  const ids = chunkIds(result.data)
  t.true(ids.includes('ANIM'))
  t.is(ids.filter((id) => id === 'ANMF').length, 3)
})

test('sweep respects minQuality', async (t) => {
  const result = await convert(fixture('rgb.jpg'), { minQuality: 30 })

  t.true(result.quality >= 30 && result.quality <= 100)
})

test('fixed quality skips the sweep', async (t) => {
  const still = await convert(fixture('rgb.jpg'), { quality: 50 })
  const animated = await convert(fixture('animated.gif'), { quality: 50, minQuality: 90 })

  t.is(still.quality, 50)
  t.is(animated.quality, 50)
})

test('throws on out-of-range options', (t) => {
  const input = fixture('rgb.jpg')

  t.throws(() => convert(input, { minQuality: 101 }), { message: /minQuality/ })
  t.throws(() => convert(input, { quality: -1 }), { message: /quality/ })
  t.throws(() => convert(input, { quality: 50.5 }), { message: /quality/ })
})

test('rejects input that is not an image', async (t) => {
  await t.throwsAsync(convert(Buffer.from('definitely not an image')), { message: /unrecognized image format/i })
})

test('rejects a truncated image', async (t) => {
  const truncated = fixture('rgba.png').subarray(0, 64)

  await t.throwsAsync(convert(truncated), { message: /failed to decode image/i })
})

test('convertFile writes the WebP to disk', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'webp-converter-'))
  t.teardown(() => rmSync(dir, { recursive: true, force: true }))
  const output = join(dir, 'out.webp')

  const result = await convertFile(join(fixturesDir, 'animated.gif'), output)

  t.is(result.frameCount, 3)
  assertIsWebp(t, readFileSync(output))
})

test('convertFile rejects a missing input', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'webp-converter-'))
  t.teardown(() => rmSync(dir, { recursive: true, force: true }))

  await t.throwsAsync(convertFile(join(dir, 'missing.png'), join(dir, 'out.webp')), { message: /not found/i })
})
