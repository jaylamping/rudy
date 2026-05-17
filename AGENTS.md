# Agents

## Cursor Cloud specific instructions

### Services overview

| Service | Command | Port | Notes |
|---------|---------|------|-------|
| **cortex** (Rust daemon) | `cargo run --manifest-path crates/Cargo.toml -p cortex` | `127.0.0.1:8443` | Runs mock CAN by default (`config/cortex.toml` → `can.mock = true`). No hardware needed. |
| **link** (Vite SPA) | `cd link && npm run dev` | `localhost:5173` | Proxies `/api/*` to cortex. |

### Key gotchas

- **`link/dist` must exist** before `cargo build -p cortex`. The `rust-embed` crate embeds the SPA from that directory at compile time. The update script creates a stub; a real build (`cd link && npm run build`) populates it fully.
- **Rust toolchain must be ≥ 1.85** (dependency `time-macros` requires `edition2024`). Run `rustup update stable` if builds fail with "feature `edition2024` is required".
- **Python test deps** (`pytest`, `pyyaml`, `xacro`, `urdfdom-py`) installed to user site-packages with `--break-system-packages`; use `python3 -m pytest tests/` (add `~/.local/bin` to PATH if `pytest` isn't found).
- **Git hooks**: `git config core.hooksPath scripts/git-hooks` — pre-commit runs `cargo fmt --check`, pre-push runs git-lfs.
- **TS type generation**: `cd link && npm run gen:types` regenerates `link/src/lib/types/` from Rust structs. Run after changing any `#[derive(TS)]` struct.

### Standard commands (see README for full docs)

```bash
# Lint / format
cargo fmt --manifest-path crates/Cargo.toml     # Rust format
cargo clippy -p cortex                          # Rust lint
cd link && npm run lint                         # ESLint
cd link && npm run typecheck                    # tsc -b --noEmit

# Tests
cd crates && cargo test -p cortex               # 340 Rust tests
cd link && npm test                             # 70 Vitest tests
python3 -m pytest tests/                        # 19 Python parity tests

# Dev servers
cargo run --manifest-path crates/Cargo.toml -p cortex   # backend (from repo root)
cd link && npm run dev                                  # frontend
cd link && npm run dev:stack                            # both via concurrently
```
