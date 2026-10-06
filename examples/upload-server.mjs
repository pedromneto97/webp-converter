// HTTP endpoint that converts an uploaded image to WebP, with the limits SECURITY.md recommends
// for untrusted input: a maximum body size and a cap on concurrent conversions.
//
//   npm install rust-webp-converter
//   node upload-server.mjs
//   curl --data-binary @photo.jpg http://localhost:3000/convert -o photo.webp
import { createServer } from 'node:http'

import { convert } from 'rust-webp-converter'

const MAX_UPLOAD_BYTES = 10 * 1024 * 1024
// Conversions share the libuv threadpool (default size 4) with fs, dns and crypto.
const MAX_CONCURRENT_CONVERSIONS = 2

let running = 0

async function readBody(request, limit) {
  const chunks = []
  let size = 0
  for await (const chunk of request) {
    size += chunk.length
    if (size > limit) {
      throw Object.assign(new Error('Upload too large'), { status: 413 })
    }
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}

const server = createServer(async (request, response) => {
  if (request.method !== 'POST' || request.url !== '/convert') {
    response.writeHead(404).end()
    return
  }
  if (running >= MAX_CONCURRENT_CONVERSIONS) {
    response.writeHead(503, { 'retry-after': '1' }).end('Busy, try again')
    return
  }

  running++
  try {
    const input = await readBody(request, MAX_UPLOAD_BYTES)
    // A fixed quality keeps the CPU cost per request predictable.
    const { data } = await convert(input, { quality: 80 })
    response.writeHead(200, { 'content-type': 'image/webp', 'content-length': data.length }).end(data)
  } catch (error) {
    // Decode failures mean the client sent something that is not a supported image.
    response.writeHead(error.status ?? 422, { 'content-type': 'text/plain' }).end(error.message)
  } finally {
    running--
  }
})

server.listen(3000, () => console.log('POST an image to http://localhost:3000/convert'))
