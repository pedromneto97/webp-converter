# Security Policy

## Supported versions

Only the latest release on npm receives security fixes. The package is pre-1.0, so fixes ship as a new patch or minor
version and are not backported.

## Reporting a vulnerability

Do not open a public issue for a security problem.

Report it privately through GitHub: [Security → Report a vulnerability](https://github.com/pedromneto97/webp-converter/security/advisories/new).
If you cannot use GitHub, email pedromneto97@gmail.com.

Please include the affected version, your platform and Node.js version, and a minimal input or script that reproduces
the problem. Expect an acknowledgement within 7 days. Confirmed issues are fixed and disclosed through a GitHub
security advisory, and the reporter is credited unless they ask otherwise.

Vulnerabilities in libwebp, the Rust `image` crate or other dependencies should also be reported upstream. Tell us as
well, so we can ship the patched version.

## What is in scope

`rust-webp-converter` is a native addon. It parses image bytes with native code (libwebp through `libwebp-sys`, plus the
Rust `image` crate and its decoders) and runs that code inside your Node.js process. Memory corruption, out-of-bounds
reads or writes, crashes triggered by crafted input, and unbounded resource use are all in scope.

## Threat model and known limitations

The library does not sandbox its input. If you convert files from untrusted sources, such as user uploads, read this
section.

- **Resource use is not capped.** Decoding limits come from the `image` crate defaults (a 512 MiB allocation limit
  for still images). A small crafted file, especially a GIF with a very large declared canvas or many frames, can use
  gigabytes of memory and a lot of CPU. Each call encodes several candidates. A low `minQuality` increases the CPU cost.
  Allocation failure aborts the process.
- **Crashes abort the process.** Conversion runs on a libuv worker thread. A panic in a decoder or an allocation
  failure on that thread terminates the whole Node.js process instead of rejecting the promise.
- **`convertFile` does no path validation.** It follows symlinks, reads the whole input into memory, and writes the
  output non-atomically. Writing to the input path replaces the input. Never pass user-controlled paths without
  validating them yourself.
- **`convert` reads the input `Buffer` from a worker thread.** Do not modify, resize or transfer the buffer until the
  returned promise settles.
- **Many formats are compiled in.** The `image` crate is built with its default features, so more parsers are exposed
  to untrusted bytes than a WebP converter strictly needs.

### Recommendations for callers

- Enforce a maximum input size and reject oversized files before calling the library.
- Run conversions of untrusted input in a separate process or container with memory and CPU limits, so a crash or
  out-of-memory kill does not take down your server.
- Limit how many conversions run at once. They share the libuv threadpool with `fs`, `dns` and `crypto`.
- Validate and canonicalize any path you pass to `convertFile`, and do not use the input path as the output path.
- Keep the package up to date.

## Supply chain

- Releases are created by release-please and published from GitHub Actions only, with npm provenance enabled. Verify
  with `npm audit signatures`.
- Prebuilt binaries are built in CI from the tagged commit, for the targets listed in `package.json` under
  `napi.targets`. The package has no install scripts and downloads nothing at install time.
- Dependencies are pinned in `Cargo.lock` and `yarn.lock` and updated through Renovate.
