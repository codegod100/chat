# Chat

Simple **Vidya** / egui LLM chat. API keys are loaded from OpenBao (`openbao.boxd.sh`, secret `ai-api-keys`) — no paste-into-settings flow for day-to-day use.

Assistant replies render as CommonMark (headings, lists, code fences).

## Run

```bash
nix run                 # cargo run (+ staged .desktop)
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

## License

MIT
