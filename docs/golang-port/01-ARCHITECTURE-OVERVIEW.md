# 01 - Architecture Overview

## What is FrankClaw?

FrankClaw is a security-hardened AI assistant gateway that connects messaging channels (Telegram, Discord, Slack, etc.) to AI model providers (OpenAI, Anthropic, Ollama) via a local WebSocket gateway. This document maps its core architecture for porting to Go with GORM.

## High-Level Data Flow

```
User Message (Channel)
    |
    v
[Channel Adapter] ──> InboundMessage
    |
    v
[Runtime / Agentic Loop]
    |
    ├── Load session transcript (medium-term memory)
    ├── Query long-term memory (vector + FTS5)
    ├── Build system prompt (templates + context)
    ├── Optimize context window (token budgeting)
    |
    v
[Model Provider] ──> CompletionRequest
    |
    ├── Failover chain (circuit breaker + retry)
    ├── Smart routing (complexity scoring)
    ├── Cost tracking (daily budget + hourly rate)
    |
    v
CompletionResponse
    |
    ├── Has tool_calls? ──YES──> [Tool Registry]
    |       |                        |
    |       |                    Execute tools
    |       |                        |
    |       |                    Tool results
    |       |                        |
    |       └── Feed results back to model (loop)
    |
    └── No tool_calls ──> Final response
            |
            v
    [Persist to transcript]
    [Send to channel]
```

## Core Crates → Go Packages Mapping

| Rust Crate | Purpose | Go Package Suggestion |
|---|---|---|
| `frankclaw-core` | Shared types, traits, errors | `pkg/core` |
| `frankclaw-runtime` | Agentic loop, context, prompts | `pkg/runtime` |
| `frankclaw-models` | LLM providers, failover, streaming | `pkg/models` |
| `frankclaw-tools` | Tool registry & implementations | `pkg/tools` |
| `frankclaw-sessions` | SQLite session store | `pkg/sessions` (GORM) |
| `frankclaw-memory` | FTS5 + vector memory | `pkg/memory` (GORM) |
| `frankclaw-crypto` | Encryption, hashing, KDF | `pkg/crypto` |
| `frankclaw-channels` | Channel adapters | `pkg/channels` |
| `frankclaw-media` | File store, SSRF protection | `pkg/media` |
| `frankclaw-cron` | Scheduled jobs | `pkg/cron` |

## Key Architectural Patterns

### 1. Trait-based Abstractions (→ Go Interfaces)
Every major subsystem is defined as an async trait in `frankclaw-core`:
- `ModelProvider` → LLM completions
- `SessionStore` → session persistence
- `ChannelPlugin` → messaging channels
- `CanvasService` → scratchpad documents
- `Fetcher`, `MessageSender`, `MemorySearch`, `AudioTranscriber`, `CronManager` → tool services

**Go equivalent:** Define interfaces in `pkg/core/interfaces.go`.

### 2. Arc + DashMap / ArcSwap (→ Go sync primitives)
- `Arc<dyn Trait>` → Go interface values (already reference types)
- `DashMap` → `sync.Map` or sharded map
- `ArcSwap` → `atomic.Value` for hot-reload config

### 3. Tokio Structured Concurrency (→ Go goroutines + context)
- `CancellationToken` → `context.Context` with cancel
- `JoinSet` → `errgroup.Group`
- `oneshot::channel` → `chan` with capacity 1
- `mpsc::channel` → buffered `chan`

### 4. Error Hierarchy (→ Go custom errors)
- `FrankClawError` has 30+ variants with `thiserror`
- Each variant maps to an HTTP status code
- `is_retryable()` method for retry logic
- **Go equivalent:** Custom error types with `errors.Is()` / `errors.As()` support

## Files That Make Up the Core

### Entry Points
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-runtime/src/lib.rs` | ~1800 | **Main agentic loop** (`Runtime::chat()`) |
| `crates/frankclaw-core/src/lib.rs` | ~50 | Module exports |

### Core Types (must port first)
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-core/src/types.rs` | ~120 | `AgentId`, `SessionKey`, `ChannelId`, `Role`, `MediaId` |
| `crates/frankclaw-core/src/model.rs` | ~270 | `CompletionMessage`, `CompletionRequest/Response`, `ModelProvider` trait, `ToolDef`, `StreamDelta` |
| `crates/frankclaw-core/src/session.rs` | ~200 | `SessionStore` trait, `SessionEntry`, `TranscriptEntry`, `SessionScoping` |
| `crates/frankclaw-core/src/error.rs` | ~150 | `FrankClawError` (30+ variants) |
| `crates/frankclaw-core/src/config.rs` | ~500+ | `FrankClawConfig` (root config struct) |
| `crates/frankclaw-core/src/channel.rs` | ~200 | `ChannelPlugin` trait, `InboundMessage`, `OutboundMessage` |
| `crates/frankclaw-core/src/hooks.rs` | ~500 | `HookRegistry`, `HookEvent`, `EventType` |
| `crates/frankclaw-core/src/auth.rs` | ~130 | `AuthMode`, `AuthRole`, rate limiting |
| `crates/frankclaw-core/src/canvas.rs` | ~60 | `CanvasService` trait, `CanvasDocument` |
| `crates/frankclaw-core/src/media.rs` | ~325 | `MediaFile`, `FileScanService` trait, SSRF `is_safe_ip()` |
| `crates/frankclaw-core/src/tool_approval.rs` | ~70 | `ApprovalRequest`, `ApprovalDecision` |
| `crates/frankclaw-core/src/tool_services.rs` | ~97 | `Fetcher`, `MessageSender`, `MemorySearch` traits |
| `crates/frankclaw-core/src/links.rs` | ~260 | URL extraction, SSRF protection |
| `crates/frankclaw-core/src/sanitize.rs` | ~226 | `sanitize_for_prompt()`, `wrap_external_content()` |
| `crates/frankclaw-core/src/api_keys.rs` | ~233 | `KeyRotator`, `ProviderKeyManager` |
| `crates/frankclaw-core/src/protocol.rs` | ~200 | WebSocket RPC frames, `Method` enum |

### Runtime (the brain)
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-runtime/src/lib.rs` | ~1800 | `Runtime::chat()` - main loop |
| `crates/frankclaw-runtime/src/context.rs` | ~200 | Context window optimization, token estimation |
| `crates/frankclaw-runtime/src/subagent.rs` | ~380 | `SubagentRegistry`, `SpawnRequest`, hierarchical execution |
| `crates/frankclaw-runtime/src/prompts.rs` | ~100 | Template rendering, compile-time prompt embedding |
| `crates/frankclaw-runtime/src/sanitize.rs` | ~6 | Re-exports from core |
| `crates/frankclaw-runtime/src/leak_detector.rs` | ~150+ | Credential leak scanning in tool outputs |
| `crates/frankclaw-runtime/prompts/*.md` | ~10 files | Prompt templates (embedded at compile time) |

### Model Providers
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-models/src/openai.rs` | 159 | OpenAI provider |
| `crates/frankclaw-models/src/anthropic.rs` | 854 | Anthropic provider (most complex) |
| `crates/frankclaw-models/src/ollama.rs` | 221 | Ollama local models |
| `crates/frankclaw-models/src/copilot.rs` | 565 | GitHub Copilot (OAuth device flow) |
| `crates/frankclaw-models/src/openai_compat.rs` | 553 | Shared OpenAI-compatible request/response |
| `crates/frankclaw-models/src/failover.rs` | 200 | Failover chain with model routing |
| `crates/frankclaw-models/src/circuit_breaker.rs` | 252 | Circuit breaker state machine |
| `crates/frankclaw-models/src/retry.rs` | 146 | Exponential backoff + retryability |
| `crates/frankclaw-models/src/routing.rs` | 898 | 13-dimension complexity scorer |
| `crates/frankclaw-models/src/cost_guard.rs` | 407 | Daily budget + hourly rate |
| `crates/frankclaw-models/src/costs.rs` | 154 | Per-model cost lookup |
| `crates/frankclaw-models/src/cache.rs` | 362+ | Response cache (TTL + LRU) |
| `crates/frankclaw-models/src/catalog.rs` | 400+ | Static model metadata |
| `crates/frankclaw-models/src/sse.rs` | 127 | SSE decoder for streaming |

### Tool System
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-tools/src/lib.rs` | 2940 | `ToolRegistry`, `ToolContext`, browser + canvas tools |
| `crates/frankclaw-tools/src/bash.rs` | 596 | Bash execution with sandbox |
| `crates/frankclaw-tools/src/file.rs` | 474 | File read/write/edit |
| `crates/frankclaw-tools/src/web.rs` | 334 | Web fetch + search |
| `crates/frankclaw-tools/src/memory.rs` | 266 | Memory get + search tools |
| `crates/frankclaw-tools/src/messaging.rs` | 254 | Message send + react |
| `crates/frankclaw-tools/src/sessions.rs` | 168 | Session inspect/list/history |
| `crates/frankclaw-tools/src/cron_tools.rs` | 221 | Cron list/add/remove |
| `crates/frankclaw-tools/src/image.rs` | 208 | Image description (vision) |
| `crates/frankclaw-tools/src/audio.rs` | 166 | Audio transcription |
| `crates/frankclaw-tools/src/pdf.rs` | 242 | PDF text extraction |
| `crates/frankclaw-tools/src/config_tools.rs` | 219 | Config inspection |
| `crates/frankclaw-tools/src/aria.rs` | 525 | Accessibility tree parsing |
| `crates/frankclaw-tools/src/browser_profiles.rs` | 221 | Browser profile management |

### Memory System
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-memory/src/lib.rs` | ~85 | `MemoryStore` trait, `ChunkEntry`, `SearchResult` |
| `crates/frankclaw-memory/src/store.rs` | ~516 | SQLite + FTS5 + vector store |
| `crates/frankclaw-memory/src/embedding.rs` | ~603 | 5 embedding providers + caching |
| `crates/frankclaw-memory/src/chunking.rs` | ~119 | Text chunking by paragraphs |
| `crates/frankclaw-memory/src/sync.rs` | ~225 | File → memory sync with hashing |

### Sessions
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-sessions/src/store.rs` | ~900+ | SQLite session store with encryption |
| `crates/frankclaw-sessions/src/migrations.rs` | ~53 | Schema DDL |

### Crypto
| File | Lines | Purpose |
|---|---|---|
| `crates/frankclaw-crypto/src/lib.rs` | ~38 | Public API + `CryptoError` |
| `crates/frankclaw-crypto/src/encryption.rs` | ~100 | ChaCha20-Poly1305 |
| `crates/frankclaw-crypto/src/keys.rs` | ~100 | Argon2id KDF + HMAC-SHA256 subkeys |
| `crates/frankclaw-crypto/src/hashing.rs` | ~88 | Password hashing (Argon2id) |
| `crates/frankclaw-crypto/src/token.rs` | ~100 | Random token gen + constant-time compare |

## Suggested Go Port Order

1. **`pkg/core`** - Types, interfaces, errors (foundation)
2. **`pkg/crypto`** - Encryption, hashing (needed by sessions)
3. **`pkg/sessions`** - GORM session store (needed by runtime)
4. **`pkg/memory`** - GORM memory store + embeddings
5. **`pkg/models`** - LLM providers + failover
6. **`pkg/tools`** - Tool registry + implementations
7. **`pkg/runtime`** - Agentic loop (depends on all above)
8. **`pkg/channels`** - Channel adapters (last, pluggable)
