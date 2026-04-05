# 05 - Model Providers

## Overview

The model provider system abstracts LLM interactions behind a common interface, with failover, circuit breaking, retry logic, streaming, cost tracking, and smart routing.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-core/src/model.rs` | 252-270 | `ModelProvider` trait |
| `crates/frankclaw-models/src/openai.rs` | 159 | OpenAI provider |
| `crates/frankclaw-models/src/anthropic.rs` | 854 | Anthropic provider (most complex) |
| `crates/frankclaw-models/src/ollama.rs` | 221 | Ollama local models |
| `crates/frankclaw-models/src/copilot.rs` | 565 | GitHub Copilot (OAuth device flow) |
| `crates/frankclaw-models/src/openai_compat.rs` | 553 | Shared OpenAI-compatible request/response builder |
| `crates/frankclaw-models/src/failover.rs` | 200 | Failover chain with model routing |
| `crates/frankclaw-models/src/circuit_breaker.rs` | 252 | Circuit breaker state machine |
| `crates/frankclaw-models/src/retry.rs` | 146 | Exponential backoff + retryability |
| `crates/frankclaw-models/src/routing.rs` | 898 | 13-dimension complexity scorer |
| `crates/frankclaw-models/src/cost_guard.rs` | 407 | Daily budget + hourly rate limiter |
| `crates/frankclaw-models/src/costs.rs` | 154 | Per-model cost lookup table |
| `crates/frankclaw-models/src/cache.rs` | 362+ | Response cache (TTL + LRU) |
| `crates/frankclaw-models/src/catalog.rs` | 400+ | Static model metadata database |
| `crates/frankclaw-models/src/sse.rs` | 127 | Server-Sent Events decoder |

## ModelProvider Interface (model.rs:252-270)

```go
type ModelProvider interface {
    ID() string
    Complete(ctx context.Context, req CompletionRequest, streamCh chan<- StreamDelta) (*CompletionResponse, error)
    ListModels(ctx context.Context) ([]ModelDef, error)
    Health(ctx context.Context) bool
}
```

## Provider Implementations

### OpenAI (openai.rs, 159 lines)

**Struct:** HTTP client + API key + base URL + model list

**`Complete()` flow:**
1. Build request body via `openai_compat.BuildRequestBody()`
2. Set `stream=true` + `stream_options.include_usage=true` if streaming
3. POST to `{base_url}/chat/completions` with `Authorization: Bearer {key}`
4. If streaming: decode SSE events → accumulate in `StreamState` → emit `StreamDelta`
5. Parse response via `openai_compat.ParseCompletionResponse()`

**Timeout:** 120s

### Anthropic (anthropic.rs, 854 lines) - MOST COMPLEX

**Struct:** HTTP client + API key + model list

**Request transformation (`build_request_body`, lines 163-279):**
- User messages → `role: "user"`, content string or multimodal array
- Assistant with tool_calls → content blocks with `type: "tool_use"` blocks
- Tool results → `role: "user"` with `type: "tool_result"` content block
- Images → base64 image content blocks with media_type
- System prompt → content block array with `cache_control: { type: "ephemeral" }`
- Tools → name validation regex: `^[a-zA-Z0-9_-]{1,128}`
- Extended thinking → `thinking.type = "enabled"` + `budget_tokens`, forces `temperature = 1`

**Headers:**
```
x-api-key: {key}
anthropic-version: 2023-06-01
content-type: application/json
```

**Streaming events (lines 384-487):**
| SSE Event | Action |
|---|---|
| `message_start` | Parse input_tokens, cache tokens |
| `content_block_start` | If tool_use → emit `ToolCallStart` |
| `content_block_delta` | Text → emit `Text`; tool input → emit `ToolCallDelta`; thinking → accumulate |
| `content_block_stop` | emit `ToolCallEnd` |
| `message_delta` | Update finish_reason + output_tokens |
| `message_stop` | Set done=true |
| `error` | Emit error delta |

**Response parsing (lines 281-329):**
- Iterates content blocks: thinking (→ `<thinking>...</thinking>` markers), text, tool_use
- Tool calls parsed from `block.input` (already JSON object, unlike OpenAI's string)

**Error classification (lines 513-559):**
| HTTP | Error |
|---|---|
| 401 | Auth failure (non-retryable) |
| 402 | Billing (non-retryable) OR rate_limit spend cap (retryable) |
| 429 | Rate limit (retryable) |
| body contains "context length" | Context overflow (non-retryable) |

### Ollama (ollama.rs, 221 lines)

**Struct:** HTTP client + normalized base URL

- Timeout: 300s (longer for local models)
- Delegates to `openai_compat` module (Ollama has OpenAI-compatible endpoint)
- URL normalization: strips `/v1` suffix for native API calls
- `list_models()`: GET `/api/tags` → parse models array
- `health()`: GET `/api/tags` check

### Copilot (copilot.rs, 565 lines)

**Struct:** GitHub token + cache path + models + inner (lazy OpenAI provider)

**Token exchange flow:**
1. Try cached Copilot token (check expiry + 60s buffer)
2. If expired/missing: exchange GitHub OAuth token → Copilot token
   - POST `https://api.github.com/copilot_internal/v2/token`
3. Cache result with 0o600 permissions + expiry
4. Extract `proxy-ep=` from semicolon-delimited token for base URL
5. Construct inner `OpenAiProvider` with Copilot token + models

**Device flow OAuth (for initial setup):**
- Client ID: `Iv1.b507a08c87ecfe98` (well-known GitHub OAuth)
- `request_device_code()` → user code + verification URL
- `poll_for_access_token()` → handles `authorization_pending`, `slow_down`, `expired_token`

### OpenAI-Compatible Shared Module (openai_compat.rs, 553 lines)

Used by: OpenAI, Ollama, Copilot

**`BuildRequestBody()`** (lines 8-122):
- System prompt as first message (role: "system")
- Assistant tool_calls:
  ```json
  {"role": "assistant", "tool_calls": [{"id": "...", "type": "function", "function": {"name": "...", "arguments": "..."}}]}
  ```
- Tool results: `{"role": "tool", "tool_call_id": "...", "content": "..."}`
- Images: multimodal array with `"type": "image_url"`, data URI format

**`ParseCompletionResponse()`** (lines 125-161):
- Extract first choice, parse content, tool_calls, usage, finish_reason

**Streaming `StreamState`** (lines 165-183):
- Accumulates: content string, tool_calls (BTreeMap by index), usage, finish_reason
- `finish()` → validates tool calls, builds `CompletionResponse`

## SSE Decoder (sse.rs, 127 lines)

Parses Server-Sent Events from HTTP streaming:

```go
type SseDecoder struct {
    buffer    []byte
    event     *string
    dataLines []string
}

func (d *SseDecoder) Push(chunk []byte) []SseEvent  // Feed bytes, get events
func (d *SseDecoder) Finish() []SseEvent             // Flush remaining
```

Handles: CRLF + LF line endings, `event:` and `data:` fields, empty line flush, comment skipping.

## Failover Chain (failover.rs, 200 lines)

```go
type FailoverChain struct {
    entries     []ProviderEntry
    retryConfig RetryConfig
}

type ProviderEntry struct {
    Provider ModelProvider
    Breaker  *CircuitBreaker
    ModelIDs []string // if set, only routes matching models here
}
```

**`Complete()` algorithm (lines 76-173):**

```
for each provider:
    if provider.ModelIDs set AND request.ModelID not in list → skip
    if circuitBreaker.IsOpen() → skip

    for attempt := 0; attempt <= maxRetries; attempt++:
        response, err = provider.Complete(request, streamCh)

        if err == nil:
            breaker.RecordSuccess()
            return response

        if alreadyStreamedData:
            return err  // can't retry mid-stream

        if isRetryable(err) AND attempt < maxRetries:
            sleep(retryBackoff(attempt))  // 1s → 2s → 4s with ±25% jitter
            continue

        breaker.RecordFailure()
        break  // try next provider

return ErrAllProvidersFailed
```

## Circuit Breaker (circuit_breaker.rs, 252 lines)

State machine for provider health:

```
Closed ──[5 consecutive failures]──> Open ──[30s timeout]──> HalfOpen
                                       ^                        |
                                       +──[any failure]─────────+
                                       |
                                   [2 successes] ──> Closed
```

**Config defaults:**
| Setting | Default |
|---|---|
| `failure_threshold` | 5 consecutive transient failures |
| `recovery_timeout` | 30 seconds |
| `half_open_successes_needed` | 2 |

```go
type CircuitBreaker struct {
    mu     sync.Mutex
    state  CircuitState // Closed, Open, HalfOpen
    config CircuitBreakerConfig
    consecutiveFailures int
    openedAt            time.Time
    halfOpenSuccesses   int
}

func (cb *CircuitBreaker) IsAllowed() bool        // false only if Open AND not ready for HalfOpen
func (cb *CircuitBreaker) RecordSuccess()          // Closes in HalfOpen after threshold
func (cb *CircuitBreaker) RecordFailure()          // Opens on threshold reached
```

## Retry Logic (retry.rs, 146 lines)

**Backoff formula:**
```
delay = 1s × 2^attempt ± 25% jitter
minimum = 100ms
sequence: ~1s → ~2s → ~4s → ~8s
```

**Retryability classification:**

| Retryable | Non-retryable |
|---|---|
| timeout, connection error | auth, unauthorized |
| rate limit, 429, 5xx | context length exceeded |
| temporary, unavailable | model not found |
| overloaded | invalid api key |

Non-retryable patterns are checked FIRST (take precedence).

## Streaming Data Flow

```
Non-streaming:
  Provider.Complete(req, nil) → CompletionResponse

Streaming:
  Provider.Complete(req, streamCh)
  │
  ├── POST with stream=true
  ├── Receive byte chunks
  ├── SseDecoder.Push(chunk) → SSE events
  ├── apply_stream_event(event) → StreamDelta
  ├── streamCh <- StreamDelta (real-time to user)
  ├── Accumulate in StreamState
  └── StreamState.Finish() → CompletionResponse
```

## Tool Call Parsing Differences

| Provider | Tool call format |
|---|---|
| **OpenAI** | `tool_calls[].function.name` + `.arguments` (JSON string) |
| **Anthropic** | Content blocks with `type: "tool_use"`, `.input` (JSON object) |
| **Ollama** | Same as OpenAI (via openai_compat) |

Both are normalized to `ToolCallResponse{id, name, arguments}` where arguments is always a JSON string.

## Go Implementation Notes

1. Use `net/http` with streaming response body for SSE
2. `io.Reader` + `bufio.Scanner` for SSE line parsing
3. Circuit breaker: use `sync.Mutex` (simple, single-purpose)
4. Failover: copy `CompletionRequest` before each attempt (messages are shared via slice)
5. Anthropic is the most complex - handle content blocks, thinking, tool_use blocks
6. For streaming with channels: close the channel when done (`StreamDelta{Type: "done"}`)
