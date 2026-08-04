# AGENTS.md

## Cursor Cloud specific instructions

`chat` is a Rust **egui/eframe** desktop LLM client (see `README.md`). It has a
Cargo *path* dependency on a sibling crate `vidya` (`vidya = { path = "../vidya" }`).

### Build / run without Nix

Nix is **not** installed in the cloud VM. Build and run directly with cargo
(the flake targets are for CI/reproducible builds only):

- Build: `cargo build`
- Run the GUI: `cargo run` (or `./target/debug/chat`)
- Test: `cargo test`
- Lint: `cargo clippy --all-targets`, `cargo fmt --check`

`cargo fmt --check` reports style diffs because the repo was formatted with the
Nix-pinned rustfmt; the cloud VM uses a newer stable rustfmt. These are cosmetic
version differences, not errors — do not "fix" them unless asked.

### The `vidya` sibling (required)

Cargo resolves `../vidya` to `/vidya`. Because `/` is not user-writable, `vidya`
is cloned to `/home/ubuntu/vidya` and `/vidya` is a symlink to it. The update
script keeps the clone fresh and recreates the symlink if missing. If a build
fails with "Unable to update /vidya" / "failed to read /vidya/Cargo.toml", the
sibling is missing — re-run: `git clone https://tangled.org/nandi.uk/vidya /home/ubuntu/vidya` then `sudo ln -sfn /home/ubuntu/vidya /vidya`.

### Toolchain

Requires Rust with **edition2024** support (a transitive dep, `az`, needs it).
The default rustup toolchain is stable (>= 1.85); 1.83 is too old and fails to
resolve dependencies.

### Running the GUI

Use the virtual display: `DISPLAY=:1 ./target/debug/chat`. Runtime needs
`libxkbcommon-x11` (installed); without it the app panics on startup with
`Library libxkbcommon-x11.so could not be loaded`.

### API keys (OpenBao) — needed to actually chat

On launch the app reads AI provider keys from OpenBao. It looks for a token in
`BAO_TOKEN` / `VAULT_TOKEN` / `~/.bao-token` (see `src/bao.rs`). This environment
provides the `OPENBAO_TOKEN` secret, which is a valid read token for the same
server. To load providers (OpenRouter is the default) and send messages, run with
`BAO_TOKEN="$OPENBAO_TOKEN"`. Without a token the UI still launches but the status
line shows no providers and chatting is disabled. The `bao`/`chat` smoke tests in
`cargo test` also require this token and hit the network + a live LLM.
