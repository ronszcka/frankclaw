# FrankClaw Parity TODO

This file tracks the remaining distance between FrankClaw and the broader OpenClaw feature surface.
It should stay current as features land, are deferred, or are explicitly dropped.

## Current Position

FrankClaw has been audited against OpenClaw's battle-tested implementation.
See `AUDIT_PLAN.md` for the full audit results across all 7 components.
It now has a working hardened core with:

- inbound/outbound assistant loop
- session persistence and optional transcript encryption
- provider failover
- DM pairing and stricter channel defaults
- local console UI
- cron reuse
- signed webhooks
- bounded tool execution
- local Canvas host
- operator onboarding and install helpers

FrankClaw is still not close to full OpenClaw breadth.
The main remaining gap is feature surface, not the core message-to-model flow.

## Implemented Core and Surfaces

- [x] Runtime-backed chat flow
- [x] Session persistence, pruning, encryption support
- [x] WebSocket gateway control plane for core methods
- [x] Local browser console UI
- [x] Pairing and inbound policy hardening
- [x] Cron execution through shared runtime
- [x] Signed webhook ingestion
- [x] Read-only and bounded model-driven tools
- [x] Local Canvas host surface
- [x] Operator health, remote exposure, onboarding, and systemd helpers
- [x] Normalized inbound media placeholders on supported channels
- [x] Chromium-backed browser session tools (`open`, `extract`, `snapshot`)
- [x] Selector-based browser actions (`click`, `type`, `wait`, `press`)
- [x] Browser session visibility and close control (`sessions`, `close`)

## Implemented Channels

- [x] Web
- [x] Telegram
- [x] Discord
- [x] Slack
- [x] Signal
- [x] WhatsApp Cloud API

## Missing or Partial vs OpenClaw

### Rich Channel Behavior

- [x] Rich attachment/media handling across supported channels
  Audited and hardened: Telegram caption overflow with fallback, parse-mode retry, thread-not-found DM fallback, message-not-modified idempotency. Discord 2000-char chunking, fatal close code handling. Slack fatal auth classification. WhatsApp message type filtering, error code classification. Signal mention ORC replacement, E.164 normalization, self-echo prevention. SSRF redirect chain validation across all media fetches.
- [x] Broader edit support beyond Telegram
- [x] Delete support where platforms allow it
- [x] Shared outbound text normalization and reply-safe formatting
- [x] Channel-specific streaming or pseudo-streaming delivery
- [x] Provider SSE streaming for OpenAI/Anthropic-backed chat turns
- [x] Provider SSE streaming for other configured providers (Ollama now streams via OpenAI-compatible SSE)
- [x] Explicit group allowlist routing on supported group-capable channels
- [x] Better reply-tag semantics across supported channels
- [x] Better WhatsApp-specific behavior beyond normalized inbound media/webhook handling and safer outbound text shaping
- [x] Broader platform-specific retry/backoff semantics

### Canvas Depth

- [x] Structured Canvas document model beyond title/body text
- [x] Session-linked Canvas workflows
- [x] Incremental Canvas patches instead of full-document replace
- [x] Multiple canvases or per-session canvases
- [x] Safer agent-driven UI blocks/components
- [x] Snapshot/export flows
- [x] A2UI-style richer host capabilities

### Tool Depth

- [x] Browser automation runtime
- [x] Browser session/profile management
- [x] Visual/browser snapshots
- [x] Safer action model for clicks/forms/navigation
- [x] Tool approvals for higher-risk tool families
- [x] More first-party tools beyond session inspection
- [x] Better tool tracing and operator visibility

### Deferred Runtime Depth

- [ ] Sandboxed agent runtime execution surface
- [ ] Vector memory backend

### Operator / Install

- [x] Docker support and documented container flow
- [x] Easier channel setup flows
- [x] Better channel/provider setup verification in `doctor`
- [x] Better deployment docs and examples
- [x] Config examples per supported channel
- [x] Service/install guidance beyond `systemd` output

### Test Coverage and Test Quality

- [x] More integration coverage across supported channels
- [x] Gateway-path coverage for authenticated web media upload/inbound flows
- [x] Better end-to-end coverage for operator flows
- [x] External-API contract fixtures for supported channels
- [x] More failure-path tests for provider failover and retries
- [x] Coverage for Canvas RPC/UI behavior
- [x] Unit and gateway coverage for Canvas export snapshots
- [x] Unit coverage for bounded Canvas component blocks
- [x] Coverage for onboarding/install helpers
- [x] Regression-focused tests for delivery metadata and session rewrites
- [ ] Live smoke coverage against real external platforms
- [x] More media-specific failure-path coverage for partial multi-attachment delivery

### Not Implemented Yet in the Current Direction

- [ ] Sandboxed agent runtime execution surface
- [ ] Vector memory backend
- [x] Provider SSE streaming outside the current OpenAI/Anthropic path (Ollama)
- [ ] Remaining long-tail media/channel edge cases listed above

### Still Missing OpenClaw Breadth

- [ ] Google Chat
- [ ] BlueBubbles / iMessage
- [ ] IRC
- [ ] Microsoft Teams
- [ ] Matrix
- [ ] Feishu
- [ ] LINE
- [ ] Mattermost
- [ ] Nextcloud Talk
- [ ] Nostr
- [ ] Synology Chat
- [ ] Tlon
- [ ] Twitch
- [ ] Zalo
- [ ] Zalo Personal
- [ ] Companion nodes and apps
- [ ] Voice

## Explicitly Lower Priority Right Now

- Wider long-tail channel parity
- Sandboxed agent runtime
- Vector memory backend
- Companion node/app surfaces
- Voice
- Distro-specific installers

## Near-Term Priority Order

1. Finish the remaining long-tail media/channel edge cases on supported channels.
2. Add more live-ish test coverage for real external platform behavior.
3. Keep sandboxed agent runtime on the deferred list until the current surface stops moving.
4. Keep vector memory backend on the deferred list until the runtime/tool surface settles.
