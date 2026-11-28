## Summary

<!-- Describe what this PR changes and why. -->

## Related Issue / RFC

<!-- e.g. Fixes #123 / Implements RFC-0003 §4 -->

## Checklist

- [ ] `cargo test --workspace` passes locally
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings
- [ ] Pure visual-layer boundary preserved (no `ls`/`cat`/`cd` aliases, no keybind hijacks, no non-visual host config keys)
- [ ] Host writes go through `kiseki-tx` / `Platform` (preserves symlinks, degrades read-only paths to `render-only`, respects `--dry-run`)
- [ ] Added or updated unit / integration tests for new behaviour
- [ ] Updated docs (`README.md`, `docs/`, or `site/`) if user-facing flags, commands, or adapters changed
