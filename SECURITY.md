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

- **Resource use is only partly capped.** Images wider or taller than 16383 px are rejected from the header, before
  decoding. Decoders run with the `image` crate's 512 MiB allocation limit, which some decoders enforce only loosely.
  libwebp's own encoder memory is not capped. CPU is not capped: a GIF with many large frames is re-encoded up to 4
  times, and a still image up to 51 times when `minQuality` is 0.
- **Allocation failure aborts the process.** A panic during conversion rejects the promise, but running out of memory
  terminates the whole Node.js process.
- **`convertFile` does no path validation.** It follows symlinks, reads the whole input into memory, and writes the
  output non-atomically. Writing to the input path replaces the input. Never pass user-controlled paths without
  validating them yourself.
- **Many formats are compiled in.** The `image` crate is built with its default features, so more parsers are exposed
  to untrusted bytes than a WebP converter strictly needs.

### Recommendations for callers

- Enforce a maximum input size and reject oversized files before calling the library.
- Run conversions of untrusted input in a separate process or container with memory and CPU limits, so a crash or
  out-of-memory kill does not take down your server.
- Limit how many conversions run at once. They share the libuv threadpool with `fs`, `dns` and `crypto`.
- Prefer a fixed `quality` for untrusted input, so each call encodes once.
- Validate and canonicalize any path you pass to `convertFile`, and do not use the input path as the output path.
- Keep the package up to date.

## Supply chain

- Releases are created by release-please and published from GitHub Actions only, through npm trusted publishing (OIDC,
  no long-lived npm token) with provenance. Verify with `npm audit signatures`.
- Prebuilt binaries are built in CI from the release commit, with `cargo build --locked` and without restored caches,
  for the targets listed in `package.json` under `napi.targets`. The package has no install scripts and downloads
  nothing at install time.
- Dependencies are pinned in `Cargo.lock` and `yarn.lock` and updated through Dependabot, which waits 3 days before it
  proposes a new version. Yarn also refuses package versions younger than 3 days and does not run dependency install
  scripts.
- GitHub Actions are pinned to commit SHAs. CI audits the workflows with zizmor and the Rust dependencies with
  `cargo deny check advisories`.
