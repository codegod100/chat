# Chat

Simple **Vidya** / egui LLM chat. API keys are loaded from OpenBao (`openbao.boxd.sh`, secret `ai-api-keys`) — no paste-into-settings flow for day-to-day use.

Assistant replies render as CommonMark (headings, lists, code fences) with
LaTeX math (`$…$`, `$$…$$`, `\(...\)`, `\[...\]`).

## Run

```bash
nix run                 # cargo build + gtk-launch uk.nandi.chat.desktop
nix develop             # interactive → cargo run
```

Needs sibling [`../vidya`](https://tangled.org/nandi.uk/vidya).

## OpenBao

On launch the app reads KV v2 path `secret/ai-api-keys`.

| Env / file | Purpose |
|------------|---------|
| `BAO_ADDR` / `VAULT_ADDR` | Server URL (default `https://openbao.boxd.sh`) |
| `BAO_TOKEN` / `VAULT_TOKEN` | Auth token |
| `~/.vault-token` or `~/.bao-token` | Token file fallback |

Known key fields → providers:

| Secret field | Provider |
|--------------|----------|
| `OPENROUTER_API_KEY` | OpenRouter (default when present; model list from API) |
| `DEEPSEEK_API_KEY` | DeepSeek |
| `OPENCODE_API_KEY` | OpenCode Zen |
| `OLLAMA_API_KEY` | Ollama Cloud |

## Use

1. Ensure a token is available (env or `~/.bao-token`).
2. Start the app — status line shows loaded providers.
3. Pick provider / model in the header.
4. Type a message; **Enter** sends, **Shift+Enter** newline.
5. **New** starts a fresh chat (current one stays on disk). **Resume** picks a past session. **Clear** wipes the view without deleting saved history.

## Sessions

Conversations are saved as JSON under `$XDG_DATA_HOME/uk.nandi.chat/sessions` (or `~/.local/share/uk.nandi.chat/sessions`). Each file holds messages plus the provider/model used.

## Android APK

Phone package (`uk.nandi.chat`, aarch64). Same chat UI; paste an OpenBao token in-app when no `~/.bao-token` is available (or `adb push` to `/data/local/tmp/bao-token`).

```bash
just apk-release          # → android/target/release/apk/chat.apk
just publish-apk          # → https://chat-apk.boxd.sh/chat.apk
```

Needs Android NDK (`ANDROID_NDK_HOME`, default `~/.local/share/android-ndk-r29`), `cargo-apk`, and `rustup target add aarch64-linux-android`.

## CI

On every push to `main`:

- **Cachix** — `nix build .#chat` and push store paths to [codegod100.cachix.org](https://codegod100.cachix.org) (`.github/workflows/cachix.yml`)
- **APK** — build aarch64 release and publish to [https://chat-apk.boxd.sh/chat.apk](https://chat-apk.boxd.sh/chat.apk) (`.github/workflows/apk.yml`); also uploads `chat.apk` as a GitHub Actions artifact

Required repository secret on [codegod100/chat](https://github.com/codegod100/chat):

| Secret | Purpose |
|--------|---------|
| `OPENBAO_TOKEN` | Read token for OpenBao; CI fetches KV secrets at runtime |

OpenBao paths (via `fetch-openbao-env.sh`):

| Path | Keys |
|------|------|
| `secret/data/cachix` | `CACHIX_AUTH_TOKEN` |
| `secret/data/chat` | `BOXD_TOKEN`, `CHAT_ANDROID_KEYSTORE_B64` |

Optional keystore fields in `secret/data/chat` (defaults: password `android`, alias `chat`):

| Key | Purpose |
|-----|---------|
| `CHAT_ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `CHAT_ANDROID_KEY_ALIAS` | Key alias |
| `CHAT_ANDROID_KEY_PASSWORD` | Key password |

Seed the keystore secret once from a local release build:

```bash
base64 -w0 ~/.android/chat-release.keystore
# → store as CHAT_ANDROID_KEYSTORE_B64 in secret/data/chat
```

Set the repo secret with: `gh secret set OPENBAO_TOKEN -R codegod100/chat`

## License

MIT
