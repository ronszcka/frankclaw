# OpenClaw Components — Remaining (Not Yet in FrankClaw)

This file tracks OpenClaw features/components that have **no functional equivalent** in the FrankClaw Rust codebase. These are candidates for future implementation or intentional exclusion.

Last updated: 2026-03-14

---

## Tier 1: Channels Not Yet Ported

### iMessage Channel (`openclaw/src/imessage/`)
- macOS-only channel via BlueBubbles or native bridge
- Monitor, send, probe, accounts, message context
- **Decision:** Low priority — macOS-only, niche user base. Consider after all Tier 2 work.

### LINE Channel (`openclaw/src/line/`)
- LINE Bot Platform integration (popular in Japan/Asia)
- Webhook-based, Flex Message templates, rich menus, quick replies
- Markdown-to-LINE format conversion
- Auto-reply delivery with text chunking
- **Decision:** Medium priority for Asian markets. Significant formatting work (Flex templates).

---

## Tier 2: Features Worth Implementing

### ACP — Agent Client Protocol (`openclaw/src/acp/`)
- Bi-directional agent-gateway protocol
- ACP client/server with persistent bindings
- Session-scoped route resolution
- Tool permission resolution and safety validation
- Event translation between internal and ACP wire formats
- **Decision:** Important for multi-agent orchestration. FrankClaw has subagent support but not the ACP wire protocol.

### TUI — Terminal User Interface (`openclaw/src/tui/`)
- Interactive chat REPL with slash commands
- Gateway message sending/receiving
- Session management and navigation
- Terminal formatting (messages, status, overlays)
- Progress line handling, OSC8 hyperlinks
- **Decision:** FrankClaw has a basic REPL (`frankclaw-cli/src/repl.rs`). A richer TUI with `ratatui` would be valuable.

### Daemon — Cross-Platform Service Management (`openclaw/src/daemon/`)
- LaunchAgent/LaunchDaemon plist generation (macOS)
- Systemd unit file generation (Linux)
- Windows Task Scheduler integration
- Service start/stop/restart/inspect
- **Decision:** FrankClaw has `start`/`stop` with PID files. Missing: native service installer generation (systemd units, launchd plists).

### Plugin Management (`openclaw/src/plugins/`)
- Plugin discovery from disk and bundled sources
- Plugin CLI command registration
- Hook runner for plugin events
- Plugin enable/disable state management
- Config schema merging from plugins
- **Decision:** FrankClaw has `frankclaw-plugin-sdk` (trait definitions) but NOT full plugin lifecycle management (discovery, loading, enable/disable). Implement a plugin loader.

### TTS — Text-to-Speech (`openclaw/src/tts/`)
- OpenAI TTS (voice synthesis)
- ElevenLabs TTS integration
- Microsoft Edge TTS
- Audio format inference and cleanup
- Text summarization before synthesis
- Model/voice validation
- **Decision:** Previously marked as "explicitly skipped." Reconsider if voice output becomes a requirement.

---

## Tier 3: Partial / Mostly Covered — Gaps Only

### Wizard / Onboarding (`openclaw/src/wizard/`)
- Guided interactive onboarding flow
- Secret input collection (API keys, tokens)
- Gateway config prompts (port, auth, bind address)
- Risk acknowledgment and security guidance
- Config finalization and saving
- **FrankClaw status:** Has `setup` command with provider selection and key input. Missing: guided gateway config prompts, risk acknowledgment flow.

### Browser Extensions (`openclaw/src/browser/`)
- `extension-relay.ts` — browser extension relay for out-of-band auth
- `profiles.ts` — browser user data directory management
- **FrankClaw status:** Has CDP browser tools (navigate, screenshot, click, type, evaluate, scroll, aria_snapshot). Missing: extension relay, profile management.

### Provider Auth — OAuth Flows (`openclaw/src/providers/`)
- Alibaba Qwen portal OAuth
- Kilocode provider utilities
- Google function calling parameter validation
- **FrankClaw status:** Has API key auth for all providers plus GitHub Copilot OAuth device flow. Missing: portal auth (Qwen).

### Secrets Audit (`openclaw/src/secrets/`)
- Auth profiles scanning and credential matrix
- **FrankClaw status:** Has `audit` command with 19 AuditCode categories, plaintext .env detection, ref shadowing detection, legacy credential scanning, JSON output. Missing: auth profile credential matrix.

### Memory — Batch Embedding Providers (`openclaw/src/memory/`)
- Batch embedding via Gemini, Voyage AI
- Batch HTTP with retry and status polling
- Embedding model normalization per provider
- **FrankClaw status:** Has SQLite FTS5 + cosine vector search, OpenAI/Ollama embeddings. Missing: Gemini and Voyage batch embedding providers.

### Markdown Rendering (`openclaw/src/markdown/`)
- IR (Intermediate Representation) with style/link metadata
- Multi-format output (ANSI, plain text, WhatsApp, LINE-specific)
- Table, code fence, and code span extraction
- Frontmatter parsing
- **FrankClaw status:** Uses markdown in prompts and basic rendering. Missing: rich ANSI terminal rendering, channel-specific markdown conversion.

---

## Tier 4: Intentionally Excluded / Not Needed

### `compat/` — Legacy Name Mapping
- Maps `clawdbot`/`moltbot` → `openclaw`
- **Not needed:** FrankClaw has no legacy names to support.

### `docs/` — Documentation Tests
- Test-only (slash command doc generation)
- **Not needed:** Documentation approach differs in Rust.

### `i18n/` — i18n Registry Tests
- Test-only stub; actual locales in `docs/locales/`
- **Not needed:** FrankClaw uses `rust-i18n` with its own locale files.

### `infra/` — Node.js Infrastructure
- Binary availability checks, archive handling, Bonjour/mDNS discovery
- **Partially covered:** SSRF protection in `frankclaw-media`, backoff in `frankclaw-models`. Bonjour/mDNS not implemented (niche).

### `node-host/` — Node.js Command Execution
- System command invocation with approval, browser automation
- **Covered by:** `frankclaw-tools/src/bash.rs` and browser tools.

### `process/` — Node.js Process Management
- `execFile`, command queue, process tree killing
- **Covered by:** `frankclaw-tools/src/bash.rs` with PTY/process management.

### `scripts/` — Build/CI Scripts
- Canvas/A2UI copy, CI scope detection
- **Not needed:** Rust uses `build.rs` and Cargo.

### `shared/` — Shared Types
- Chat content, envelope, message content blocks, device auth
- **Covered by:** `frankclaw-core` types and traits.

### `terminal/` — Terminal Styling
- Lobster palette, ANSI colors, hyperlinks, table rendering
- **Covered by:** `frankclaw-cli/src/terminal.rs` — color palette, severity badges, ANSI table rendering, `NO_COLOR`/`TERM=dumb` support. Missing: OSC8 hyperlinks.

### `test-helpers/` and `test-utils/` — Test Infrastructure
- SSRF mocks, workspace creation, command runner, fetch mock
- **Covered by:** Rust test utilities in each crate.

### `types/` — TypeScript Type Stubs
- `.d.ts` for untyped npm packages
- **Not needed:** Rust has its own type system.

### `utils/` — General Utilities
- Array chunking, delivery context, boolean parsing, account ID normalization
- **Covered by:** Distributed across FrankClaw crates.

### `web/` — WhatsApp Web Adapter
- WhatsApp Web browser automation, QR code login
- **Covered by:** `frankclaw-channels/src/whatsapp.rs` uses WhatsApp Business API (not web automation).

---

## Implementation Priority

| # | Component | Effort | Value | Status |
|---|-----------|--------|-------|--------|
| 1 | Plugin management (loader/lifecycle) | Medium | High | Open |
| 2 | Daemon service installers (systemd/launchd) | Low | Medium | Open |
| 3 | TUI with ratatui | Medium | Medium | Open |
| 4 | LINE channel | High | Medium (Asia) | Open |
| 5 | ACP protocol | High | Medium | Open |
| 6 | ~~Browser AI snapshots~~ | ~~Low~~ | ~~Low~~ | **Done** |
| 7 | ~~Provider OAuth flows (Copilot)~~ | ~~Medium~~ | ~~Low~~ | **Done** |
| 8 | TTS providers | Medium | Low | Open |
| 9 | iMessage channel | Medium | Low | Open |
| 10 | Batch embedding (Gemini/Voyage) | Low | Low | Open |
| 11 | Rich markdown rendering | Medium | Low | Open |
| 12 | ~~Secrets audit matrix~~ | ~~Low~~ | ~~Low~~ | **Done** |
| 13 | Wizard improvements | Low | Low | Open |
