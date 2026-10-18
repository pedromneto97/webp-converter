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
  const declared = Number(request.headers['content-length'])
  if (declared > limit) {
    throw Object.assign(new Error('Upload too large'), { status: 413 })
  }

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
    // Close the connection so an unread body cannot keep the socket busy.
    response.writeHead(404, { connection: 'close' }).end()
    return
  }

  let input
  try {
    // Read the body before taking a conversion slot, so a slow upload holds a socket, not a slot.
    input = await readBody(request, MAX_UPLOAD_BYTES)
  } catch (error) {
    // The rest of the body is never read, so close the connection instead of draining it.
    response.writeHead(error.status ?? 400, { 'content-type': 'text/plain', connection: 'close' }).end(error.message)
    return
  }

  if (running >= MAX_CONCURRENT_CONVERSIONS) {
    response.writeHead(503, { 'retry-after': '1' }).end('Busy, try again')
    return
  }

  running++
  try {
    // A fixed quality keeps the CPU cost per request predictable.
    const { data } = await convert(input, { quality: 80 })
    response.writeHead(200, { 'content-type': 'image/webp', 'content-length': data.length }).end(data)
  } catch (error) {
    // Decode failures mean the client sent something that is not a supported image.
    response.writeHead(422, { 'content-type': 'text/plain' }).end(error.message)
  } finally {
    running--
  }
})

// Bound how long a client may take to send its headers and body.
server.headersTimeout = 10_000
server.requestTimeout = 30_000

server.listen(3000, () => console.log('POST an image to http://localhost:3000/convert'))
