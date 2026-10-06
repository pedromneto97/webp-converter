<!--
PR title MUST follow Conventional Commits. release-please reads it (squash merge uses it as the commit
message) to pick the version bump and write the changelog.

  <type>[optional scope][!]: <description>

Examples:
  feat: add resize option
  fix: reject GIFs wider than 16383 px
  docs: document error codes
  feat!: rename `minQuality` to `minimumQuality`

Types: feat (minor), fix (patch), perf (patch), docs, refactor, test, build, ci, chore.
Add `!` or a `BREAKING CHANGE:` footer for a breaking change.
-->

## Summary

<!-- What changes and why. Link issues: "Closes #123". -->

## Type of change

- [ ] `feat`: new feature
- [ ] `fix`: bug fix
- [ ] `perf`: performance improvement
- [ ] `docs`: documentation only
- [ ] `refactor` / `test` / `build` / `ci` / `chore`
- [ ] Breaking change (title has `!` or body has a `BREAKING CHANGE:` footer)

## Checklist

- [ ] PR title follows Conventional Commits (`feat: My feature`)
- [ ] `yarn lint` and `cargo clippy` pass
- [ ] `yarn format` applied (CI checks `cargo fmt -- --check`)
- [ ] `yarn build && yarn test` and `cargo test` pass
- [ ] Tests added or updated
- [ ] If the exported Rust API changed: rebuilt and committed `index.js` and `index.d.ts` (generated, do not edit by hand)
- [ ] If behavior or options changed: updated `README.md` and doc comments in `src/lib.rs`
- [ ] No manual version bump or `CHANGELOG.md` edit (release-please does it)

## Notes for reviewers

<!-- Optional: benchmarks, trade-offs, platforms tested, follow-ups. -->
