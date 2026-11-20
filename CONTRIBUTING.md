# Contributing to GLASSDESK

Thank you for contributing to **GLASSDESK**!

## Development Standards

1. **Zero-Fake System Data**: Every Windows system indicator (battery, volume, network, keyboard layout, foreground app, SMTC music session) must come from documented Win32, COM, or WinRT APIs.
2. **Strict Type Safety**: Frontend TypeScript must pass `npm run typecheck` (`strict: true`, `noUnusedLocals: true`, `noUnusedParameters: true`) with zero warnings.
3. **Rust & SQLite Safety**: Backend changes must pass `cargo test` in `src-tauri/`. Never delete, move, or overwrite original user files referenced in `FILE HOLD`.
4. **Zero Idle CPU**: Avoid continuous `requestAnimationFrame` loops or high-frequency polling when GLASSDESK is collapsed or hidden.

## Pull Request Checklist

- [ ] `npm run build` passes cleanly
- [ ] `cargo test` in `src-tauri/` passes cleanly
- [ ] GitHub Actions `Build GLASSDESK (Windows x64)` workflow succeeds for NSIS & MSI bundles
