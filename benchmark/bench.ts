import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { Bench } from 'tinybench'

import { convert } from '../index.js'

const fixturesDir = join(dirname(fileURLToPath(import.meta.url)), '..', '__test__', 'fixtures')
const still = readFileSync(join(fixturesDir, 'rgb.jpg'))
const animated = readFileSync(join(fixturesDir, 'animated.gif'))

const b = new Bench()

b.add('still, quality sweep from 80', async () => {
  await convert(still)
})

b.add('still, fixed quality 80', async () => {
  await convert(still, { quality: 80 })
})

b.add('animated GIF, quality sweep from 80', async () => {
  await convert(animated)
})

await b.run()

console.table(b.table())
