# 02 - Core Types & Interfaces

## Overview

All shared types live in `crates/frankclaw-core/src/`. These are the foundation that every other crate depends on. In Go, these become your `pkg/core` package.

## Files & What They Define

---

### `crates/frankclaw-core/src/types.rs` (~120 LOC)

**Identifiers** - Strongly-typed string wrappers with length limits:

| Rust Type | Max Length | Description | Go Equivalent |
|---|---|---|---|
| `ChannelId` | 255 chars | Channel identifier (telegram, discord, etc.) | `type ChannelID string` |
| `AgentId` | 255 chars | Agent identifier | `type AgentID string` |
| `SessionKey` | 800 bytes | Composite: `{agent_id}:{channel}:{account_id}` | `type SessionKey string` |
| `ConnId` | - | WebSocket connection ID (u64) | `type ConnID uint64` |
| `RequestId` | - | RPC request ID (number or string) | `type RequestID interface{}` |
| `Role` | - | Enum: System, User, Assistant, Tool | `type Role string` const |
| `MediaId` | - | UUID v4 for media files | `type MediaID uuid.UUID` |

**Key Functions:**
- `clamp_id(s) -> String` - Truncates to 255 bytes (prevents abuse)
- `SessionKey::new(agent_id, channel, account_id)` - Constructs composite key
- `SessionKey::parse()` - Splits key back into components
- `MediaId::new()` - Generates new UUID v4

**Go struct suggestion:**
```go
type SessionKey string

func NewSessionKey(agentID AgentID, channel ChannelID, accountID string) SessionKey {
    return SessionKey(fmt.Sprintf("%s:%s:%s", agentID, channel, accountID))
}

func (sk SessionKey) Parse() (AgentID, ChannelID, string, error) {
    parts := strings.SplitN(string(sk), ":", 3)
    if len(parts) != 3 { return "", "", "", ErrInvalidSessionKey }
    return AgentID(parts[0]), ChannelID(parts[1]), parts[2], nil
}
```

---

### `crates/frankclaw-core/src/model.rs` (~270 LOC)

**This is the most critical file.** Defines the LLM interaction contract.

#### Enums:

| Rust Enum | Variants | Go |
|---|---|---|
| `ModelApi` | OpenaiCompletions, AnthropicMessages, GoogleGenerativeAi, Ollama, etc. | `type ModelAPI string` const |
| `InputModality` | Text, Image, Audio | `type InputModality string` const |
| `ResponseFormat` | Text, JsonObject | `type ResponseFormat string` const |
| `ToolRiskLevel` | ReadOnly, Mutating, Destructive | `type ToolRiskLevel int` const iota |
| `FinishReason` | Stop, MaxTokens, ToolUse, ContentFilter | `type FinishReason string` const |
| `StreamDelta` | Text, ToolCallStart, ToolCallDelta, ToolCallEnd, Done, Error | discriminated union → Go interface |

#### Structs:

**`ModelDef`** (line 48) - Model metadata:
```go
type ModelDef struct {
    ID              string          `json:"id"`
    Name            string          `json:"name"`
    API             ModelAPI        `json:"api"`
    Reasoning       bool            `json:"reasoning"`
    Input           []InputModality `json:"input"`
    Cost            *ModelCost      `json:"cost,omitempty"`
    ContextWindow   int             `json:"context_window"`
    MaxOutputTokens int             `json:"max_output_tokens"`
    Compat          ModelCompat     `json:"compat"`
}
```

**`ModelCost`** (line 29):
```go
type ModelCost struct {
    InputPerMTok      float64  `json:"input_per_mtok"`
    OutputPerMTok     float64  `json:"output_per_mtok"`
    CacheReadPerMTok  *float64 `json:"cache_read_per_mtok,omitempty"`
    CacheWritePerMTok *float64 `json:"cache_write_per_mtok,omitempty"`
}
```

**`ModelCompat`** (line 38) - Capability flags:
```go
type ModelCompat struct {
    SupportsTools   bool `json:"supports_tools"`
    Vision          bool `json:"vision"`
    Streaming       bool `json:"streaming"`
    JsonMode        bool `json:"json_mode"`
    SystemMessage   bool `json:"system_message"`
}
```

**`ImageContent`** (line 61):
```go
type ImageContent struct {
    MimeType string `json:"mime_type"` // e.g. "image/jpeg"
    Data     string `json:"data"`      // base64 encoded
}
```

**`CompletionMessage`** (line 71) - **THE core message type**:
```go
type CompletionMessage struct {
    Role         Role               `json:"role"`
    Content      string             `json:"content"`
    ToolCalls    []ToolCallResponse `json:"tool_calls,omitempty"`
    ToolCallID   *string            `json:"tool_call_id,omitempty"`
    ImageContent []ImageContent     `json:"image_content,omitempty"`
}
```

Constructors in Rust (implement as Go functions):
- `CompletionMessage::text(role, content)` - Simple text message
- `CompletionMessage::with_images(role, content, images)` - With vision
- `CompletionMessage::assistant_tool_calls(content, tool_calls)` - Assistant requesting tools
- `CompletionMessage::tool_result(tool_call_id, content)` - Tool response
- `CompletionMessage::tool_result_with_images(tool_call_id, content, images)` - Tool response with images

**`CompletionRequest`** (line 158):
```go
type CompletionRequest struct {
    ModelID            string              `json:"model_id"`
    Messages           []CompletionMessage `json:"messages"`
    MaxTokens          *int                `json:"max_tokens,omitempty"`
    Temperature        *float64            `json:"temperature,omitempty"`
    System             *string             `json:"system,omitempty"`
    Tools              []ToolDef           `json:"tools,omitempty"`
    ThinkingBudget     *int                `json:"thinking_budget,omitempty"`
    ParallelToolCalls  *bool               `json:"parallel_tool_calls,omitempty"`
    Seed               *int                `json:"seed,omitempty"`
    ResponseFormat     *ResponseFormat     `json:"response_format,omitempty"`
    ReasoningEffort    *string             `json:"reasoning_effort,omitempty"`
}
```

**`CompletionResponse`** (line 226):
```go
type CompletionResponse struct {
    Content      string             `json:"content"`
    ToolCalls    []ToolCallResponse `json:"tool_calls"`
    Usage        Usage              `json:"usage"`
    FinishReason FinishReason       `json:"finish_reason"`
}
```

**`ToolDef`** (line 196):
```go
type ToolDef struct {
    Name        string          `json:"name"`
    Description string          `json:"description"`
    Parameters  json.RawMessage `json:"parameters"` // JSON Schema
    RiskLevel   ToolRiskLevel   `json:"risk_level"`
}
```

**`ToolCallResponse`** (line 235):
```go
type ToolCallResponse struct {
    ID        string `json:"id"`
    Name      string `json:"name"`
    Arguments string `json:"arguments"` // JSON string
}
```

**`Usage`** (line 217):
```go
type Usage struct {
    InputTokens      int  `json:"input_tokens"`
    OutputTokens     int  `json:"output_tokens"`
    CacheReadTokens  *int `json:"cache_read_tokens,omitempty"`
    CacheWriteTokens *int `json:"cache_write_tokens,omitempty"`
}
```

**`StreamDelta`** (line 206) - Streaming events:
```go
type StreamDelta struct {
    Type      string  `json:"type"` // "text", "tool_call_start", "tool_call_delta", "tool_call_end", "done", "error"
    Text      string  `json:"text,omitempty"`
    ID        string  `json:"id,omitempty"`
    Name      string  `json:"name,omitempty"`
    Arguments string  `json:"arguments,omitempty"`
    Usage     *Usage  `json:"usage,omitempty"`
    Error     string  `json:"error,omitempty"`
}
```

#### Trait → Interface:

**`ModelProvider`** (line 252):
```go
type ModelProvider interface {
    ID() string
    Complete(ctx context.Context, req CompletionRequest, streamCh chan<- StreamDelta) (*CompletionResponse, error)
    ListModels(ctx context.Context) ([]ModelDef, error)
    Health(ctx context.Context) bool
}
```

---

### `crates/frankclaw-core/src/session.rs` (~200 LOC)

**`SessionScoping`** (line 10):
```go
type SessionScoping string
const (
    ScopingMain           SessionScoping = "main"
    ScopingPerPeer        SessionScoping = "per_peer"
    ScopingPerChannelPeer SessionScoping = "per_channel_peer"
    ScopingGlobal         SessionScoping = "global"
)
```

**`SessionResetPolicy`** (line 65):
```go
type SessionResetPolicy struct {
    DailyAtHour    *int `json:"daily_at_hour,omitempty"`    // 0-23 UTC
    IdleTimeoutSec *int `json:"idle_timeout_secs,omitempty"`
    MaxEntries     *int `json:"max_entries,omitempty"`
}
```

**`SessionEntry`** (line 107) - GORM model:
```go
type SessionEntry struct {
    Key           string          `gorm:"primaryKey" json:"key"`
    AgentID       string          `json:"agent_id"`
    Channel       string          `json:"channel"`
    AccountID     string          `json:"account_id"`
    Scoping       SessionScoping  `json:"scoping"`
    ThreadID      *string         `json:"thread_id,omitempty"`
    Metadata      datatypes.JSON  `json:"metadata"`
    CreatedAt     time.Time       `json:"created_at"`
    LastMessageAt *time.Time      `json:"last_message_at,omitempty"`
}
```

**`TranscriptEntry`** (line 121) - GORM model:
```go
type TranscriptEntry struct {
    SessionKey string         `gorm:"primaryKey" json:"session_key"`
    Seq        uint64         `gorm:"primaryKey" json:"seq"`
    Role       Role           `json:"role"`
    Content    []byte         `json:"content"` // encrypted if enabled
    Metadata   datatypes.JSON `json:"metadata,omitempty"`
    Timestamp  time.Time      `json:"timestamp"`
}
```

**`SessionStore`** trait (line 131) → Go interface:
```go
type SessionStore interface {
    Get(ctx context.Context, key SessionKey) (*SessionEntry, error)
    Upsert(ctx context.Context, entry *SessionEntry) error
    Delete(ctx context.Context, key SessionKey) error
    List(ctx context.Context, agentID AgentID, limit, offset int) ([]SessionEntry, error)
    AppendTranscript(ctx context.Context, key SessionKey, entry *TranscriptEntry) error
    GetTranscript(ctx context.Context, key SessionKey, limit int, beforeSeq *uint64) ([]TranscriptEntry, error)
    ClearTranscript(ctx context.Context, key SessionKey) error
    Maintenance(ctx context.Context, config PruningConfig) (uint64, error)
}
```

---

### `crates/frankclaw-core/src/error.rs` (~150 LOC)

**`FrankClawError`** - 30+ variants. Key ones for Go:

```go
var (
    ErrAuthRequired       = errors.New("authentication required")
    ErrAuthFailed         = errors.New("authentication failed")
    ErrRateLimited        = errors.New("rate limited")
    ErrForbidden          = errors.New("forbidden")
    ErrSessionNotFound    = errors.New("session not found")
    ErrAgentNotFound      = errors.New("agent not found")
    ErrModelNotFound      = errors.New("model not found")
    ErrAllProvidersFailed = errors.New("all providers failed")
    ErrTurnCancelled      = errors.New("turn cancelled")
    ErrInvalidRequest     = errors.New("invalid request")
    ErrRequestTooLarge    = errors.New("request too large")
    ErrMediaTooLarge      = errors.New("media too large")
    ErrMalwareDetected    = errors.New("malware detected")
    ErrShuttingDown       = errors.New("shutting down")
)

// Each error has:
func StatusCode(err error) int     // HTTP-like status code
func IsRetryable(err error) bool   // Should client retry?
```

---

### `crates/frankclaw-core/src/channel.rs` (~200 LOC)

**`ChannelCapabilities`** (line 8):
```go
type ChannelCapabilities struct {
    Threads       bool `json:"threads"`
    Groups        bool `json:"groups"`
    Attachments   bool `json:"attachments"`
    Edit          bool `json:"edit"`
    Delete        bool `json:"delete"`
    Reactions     bool `json:"reactions"`
    Streaming     bool `json:"streaming"`
    Voice         bool `json:"voice"`
    InlineButtons bool `json:"inline_buttons"`
}
```

**`InboundMessage`** (line 31):
```go
type InboundMessage struct {
    Channel           ChannelID          `json:"channel"`
    AccountID         string             `json:"account_id"`
    SenderID          string             `json:"sender_id"`
    SenderName        *string            `json:"sender_name,omitempty"`
    ThreadID          *string            `json:"thread_id,omitempty"`
    IsGroup           bool               `json:"is_group"`
    IsMention         bool               `json:"is_mention"`
    Text              *string            `json:"text,omitempty"`
    Attachments       []InboundAttachment `json:"attachments"`
    PlatformMessageID *string            `json:"platform_message_id,omitempty"`
    Timestamp         time.Time          `json:"timestamp"`
}
```

**`ChannelPlugin`** trait → interface:
```go
type ChannelPlugin interface {
    ID() ChannelID
    Capabilities() ChannelCapabilities
    Label() string
    Start(ctx context.Context, inboundCh chan<- InboundMessage) error
    Stop(ctx context.Context) error
    Health(ctx context.Context) HealthStatus
    Send(ctx context.Context, msg OutboundMessage) (SendResult, error)
    // Optional methods with default no-op implementations
    EditMessage(ctx context.Context, target EditMessageTarget, newText string) error
    DeleteMessage(ctx context.Context, target DeleteMessageTarget) error
    SendReaction(ctx context.Context, channel ChannelID, accountID, to string, threadID *string, messageID, emoji string) error
}
```

---

### `crates/frankclaw-core/src/tool_services.rs` (~97 LOC)

Service abstractions for tools:

```go
type Fetcher interface {
    Fetch(ctx context.Context, url string) (*FetchedContent, error)
}

type FetchedContent struct {
    Bytes       []byte `json:"bytes"`
    ContentType string `json:"content_type"`
    FinalURL    string `json:"final_url"`
}

type MessageSender interface {
    SendText(ctx context.Context, channel ChannelID, accountID, to, text string, threadID, replyTo *string) (string, error)
    SendReaction(ctx context.Context, channel ChannelID, accountID, to string, threadID *string, messageID, emoji string) error
}

type MemorySearch interface {
    Search(ctx context.Context, query string, limit int) ([]MemorySearchResult, error)
    ListSources(ctx context.Context) ([]map[string]interface{}, error)
}

type MemorySearchResult struct {
    Source    string  `json:"source"`
    Text     string  `json:"text"`
    Score    float32 `json:"score"`
    LineStart int    `json:"line_start"`
    LineEnd   int    `json:"line_end"`
}

type AudioTranscriber interface {
    Transcribe(ctx context.Context, data []byte, mime, filename string) (string, error)
}

type CronManager interface {
    ListJobs(ctx context.Context) ([]map[string]interface{}, error)
    AddJob(ctx context.Context, id, schedule string, agentID AgentID, sessionKey SessionKey, prompt string, enabled bool) error
    RemoveJob(ctx context.Context, id string) (bool, error)
}
```

---

### `crates/frankclaw-core/src/tool_approval.rs` (~70 LOC)

```go
type ApprovalRequest struct {
    ApprovalID string        `json:"approval_id"`
    ToolName   string        `json:"tool_name"`
    ToolArgs   string        `json:"tool_args"` // JSON
    RiskLevel  ToolRiskLevel `json:"risk_level"`
    SessionKey SessionKey    `json:"session_key"`
    AgentID    AgentID       `json:"agent_id"`
}

type ApprovalDecision string
const (
    ApprovalAllowOnce   ApprovalDecision = "allow_once"
    ApprovalAllowAlways ApprovalDecision = "allow_always"
    ApprovalDeny        ApprovalDecision = "deny"
)
```

---

### `crates/frankclaw-core/src/api_keys.rs` (~233 LOC)

**`KeyRotator`** - Round-robin key selection with backoff:
```go
type KeyRotator struct {
    mu      sync.Mutex
    keys    []keyEntry
    current int
}

type keyEntry struct {
    Key        string // or SecretString equivalent
    Failures   int
    CoolingOff bool
    CooldownAt time.Time
}

func (kr *KeyRotator) Select() (string, error)           // Next available key
func (kr *KeyRotator) MarkSuccess()                       // Reset failure count
func (kr *KeyRotator) MarkFailure(reason FailureReason)   // Increment + cooldown
func (kr *KeyRotator) AvailableCount() int
```

**`ProviderKeyManager`** - Multi-provider registry:
```go
type ProviderKeyManager struct {
    mu        sync.RWMutex
    rotators  map[string]*KeyRotator // keyed by provider name
}

func (pkm *ProviderKeyManager) Register(provider string, keys []string)
func (pkm *ProviderKeyManager) Select(provider string) (string, error)
func (pkm *ProviderKeyManager) MarkSuccess(provider string)
func (pkm *ProviderKeyManager) MarkFailure(provider string, reason FailureReason)
```

---

## Interactions Between Files

```
types.rs ──> Used by EVERYTHING (AgentId, SessionKey, Role)
    │
model.rs ──> Used by runtime (CompletionMessage, CompletionRequest)
    │         Used by models crate (ModelProvider trait)
    │         Used by tools crate (ToolDef, ToolRiskLevel)
    │
session.rs ──> Used by runtime (SessionStore for persistence)
    │          Used by sessions crate (implements SessionStore)
    │
error.rs ──> Used by EVERYTHING (Result<T> type alias)
    │
channel.rs ──> Used by channels crate (ChannelPlugin trait)
    │           Used by runtime (InboundMessage processing)
    │
tool_services.rs ──> Used by tools crate (Fetcher, MemorySearch, etc.)
    │
tool_approval.rs ──> Used by runtime (approval flow in chat loop)
    │
api_keys.rs ──> Used by models crate (key rotation per provider)
    │
hooks.rs ──> Used by runtime (fire lifecycle events)
    │
sanitize.rs ──> Used by runtime (sanitize all LLM inputs)
    │
links.rs ──> Used by tools crate (URL extraction + SSRF)
```
