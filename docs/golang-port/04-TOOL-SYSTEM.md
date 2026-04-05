# 04 - Tool System

## Overview

The tool system allows the LLM to invoke external actions (read files, run commands, search memory, send messages, etc.). Tools are defined, registered, validated, and executed through a central registry.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-tools/src/lib.rs` | 2940 | `ToolRegistry`, `ToolContext`, `Tool` trait, browser + canvas tools |
| `crates/frankclaw-tools/src/bash.rs` | 596 | Bash execution with sandbox policies |
| `crates/frankclaw-tools/src/file.rs` | 474 | File read/write/edit with path validation |
| `crates/frankclaw-tools/src/web.rs` | 334 | Web fetch + search |
| `crates/frankclaw-tools/src/memory.rs` | 266 | Memory get + semantic search |
| `crates/frankclaw-tools/src/messaging.rs` | 254 | Message send + react via channels |
| `crates/frankclaw-tools/src/sessions.rs` | 168 | Session inspect/list/history |
| `crates/frankclaw-tools/src/cron_tools.rs` | 221 | Cron list/add/remove |
| `crates/frankclaw-tools/src/image.rs` | 208 | Image description (vision models) |
| `crates/frankclaw-tools/src/audio.rs` | 166 | Audio transcription |
| `crates/frankclaw-tools/src/pdf.rs` | 242 | PDF text extraction |
| `crates/frankclaw-tools/src/config_tools.rs` | 219 | Config inspection (with secret redaction) |
| `crates/frankclaw-tools/src/aria.rs` | 525 | ARIA accessibility tree parsing |
| `crates/frankclaw-tools/src/browser_profiles.rs` | 221 | Browser profile management |
| `crates/frankclaw-core/src/model.rs` | 181-203 | `ToolDef`, `ToolRiskLevel` |
| `crates/frankclaw-core/src/tool_approval.rs` | 1-70 | `ApprovalRequest`, `ApprovalDecision` |
| `crates/frankclaw-core/src/tool_services.rs` | 1-97 | Service abstractions (Fetcher, MemorySearch, etc.) |

## Core Abstractions

### Tool Trait (lib.rs:79-84)

```go
type Tool interface {
    Definition() ToolDef
    Invoke(ctx context.Context, args map[string]interface{}, toolCtx ToolContext) (interface{}, error)
}
```

### ToolDef (model.rs:196)

```go
type ToolDef struct {
    Name        string          `json:"name"`
    Description string          `json:"description"`
    Parameters  json.RawMessage `json:"parameters"` // JSON Schema
    RiskLevel   ToolRiskLevel   `json:"risk_level"`
}
```

### ToolRiskLevel (model.rs:181)

Three levels determine approval behavior:

| Level | Auto-approved | Examples |
|---|---|---|
| `ReadOnly` | Always | file_read, web_fetch, memory_search |
| `Mutating` | Only if approval=mutating | bash, file_write, browser_click, message_send |
| `Destructive` | Only if approval=destructive | cron_remove |

### ToolContext (lib.rs:47-69)

Everything a tool needs to do its job:

```go
type ToolContext struct {
    AgentID          AgentID
    SessionKey       *SessionKey
    Sessions         SessionStore
    Canvas           CanvasService       // optional
    Fetcher          Fetcher             // optional
    Channels         MessageSender       // optional
    Cron             CronManager         // optional
    MemorySearch     MemorySearch        // optional
    AudioTranscriber AudioTranscriber    // optional
    Config           *FrankClawConfig    // optional
    Workspace        string              // optional, base path for file tools
}
```

### ToolOutput (lib.rs:71-77)

```go
type ToolOutput struct {
    Name         string        `json:"name"`
    Output       interface{}   `json:"output"`
    ImageContent []ImageContent `json:"image_content,omitempty"` // for vision models
}
```

## Tool Registry (lib.rs:86-256)

### Structure

```go
type ToolRegistry struct {
    tools  map[string]Tool
    policy ToolPolicy
}

type ToolPolicy struct {
    ApprovalLevel ApprovalLevel
    ApprovedTools map[string]bool // tools always approved regardless of risk
}

type ApprovalLevel int
const (
    ApprovalReadOnly    ApprovalLevel = iota // default
    ApprovalMutating                         // +mutating auto-approved
    ApprovalDestructive                      // everything auto-approved
)
```

### Key Methods

```go
// Register a tool
func (r *ToolRegistry) Register(tool Tool)

// Get tool definitions for allowed tools (sent to LLM)
func (r *ToolRegistry) Definitions(allowedNames []string) ([]ToolDef, error)

// Invoke with full validation
func (r *ToolRegistry) InvokeAllowed(
    allowedTools []string,
    name string,
    args map[string]interface{},
    ctx ToolContext,
) (*ToolOutput, error)
```

### InvokeAllowed Flow (lib.rs:215-256)

```
1. Check tool name is in agent's allowed list → "tool not allowed for agent"
2. Check tool exists in registry → "unknown tool"
3. Check risk level against policy → "requires {level} approval"
4. Call tool.Invoke(args, ctx)
5. Extract _image_content from output (for vision models)
6. Return ToolOutput
```

### Built-in Tool Registration (lib.rs:130-189)

`with_builtins()` registers ALL tools. In Go:

```go
func NewToolRegistry() *ToolRegistry {
    r := &ToolRegistry{tools: make(map[string]Tool)}
    // Session & Config
    r.Register(&SessionInspectTool{})
    r.Register(&SessionsListTool{})
    r.Register(&SessionsHistoryTool{})
    r.Register(&ConfigGetTool{})
    r.Register(&AgentsListTool{})
    // File
    r.Register(&FileReadTool{})
    r.Register(&FileWriteTool{})
    r.Register(&FileEditTool{})
    // Web
    r.Register(&WebFetchTool{})
    r.Register(&WebSearchTool{})
    // Memory
    r.Register(&MemoryGetTool{})
    r.Register(&MemorySearchTool{})
    // Messaging
    r.Register(&MessageSendTool{})
    r.Register(&MessageReactTool{})
    // Cron
    r.Register(&CronListTool{})
    r.Register(&CronAddTool{})
    r.Register(&CronRemoveTool{})
    // Media
    r.Register(&ImageDescribeTool{})
    r.Register(&AudioTranscribeTool{})
    r.Register(&PdfReadTool{})
    // Browser (10+ tools)
    r.Register(&BrowserOpenTool{})
    // ... etc
    // Bash
    r.Register(NewBashTool(policy, sandbox))
    return r
}
```

## Tool Risk Assignment (lib.rs:357-367)

Hardcoded mapping of tool names to risk levels:

```go
func ToolRiskLevelFor(name string) ToolRiskLevel {
    switch name {
    case "browser_click", "browser_type", "browser_press",
         "bash", "file_write", "file_edit",
         "message_send", "message_react",
         "cron_add", "canvas_set", "canvas_append", "canvas_clear":
        return Mutating
    case "cron_remove":
        return Destructive
    default:
        return ReadOnly
    }
}
```

## Individual Tools Detail

### BashTool (bash.rs, 596 lines)

**Most complex tool.** Executes shell commands with sandboxing.

```go
type BashTool struct {
    Policy  BashPolicy
    Sandbox SandboxMode
}
```

**BashPolicy** (from env `FRANKCLAW_BASH_POLICY`):
- `DenyAll` (default) - No commands allowed
- `AllowAll` - No restrictions (dangerous)
- `Allowlist([]string)` - Only specific binaries

**SandboxMode** (from env `FRANKCLAW_SANDBOX`):
- `None` - Direct execution
- `AiJail` - bubblewrap + landlock isolation
- `AiJailLockdown` - Read-only FS, no network

**Security:** In allowlist mode, rejects shell metacharacters: `;|&\`$(){}!<>` and newlines.

**Parameters:** `command` (required), `workdir` (optional), `timeout` (default 120s, max 600s)
**Limits:** Max 200KB output, default 120s timeout

### FileReadTool (file.rs, lines 90-184)

**Parameters:** `path` (relative to workspace), `offset` (line, default 0), `limit` (1-10000, default 2000)
**Limits:** Max 200KB per read
**Security:** Rejects absolute paths, `..` components, validates symlinks don't escape workspace

### FileWriteTool / FileEditTool (file.rs)

**Risk:** Mutating
**Limits:** Max 1MB content
**Security:** Same path validation as FileReadTool

### WebFetchTool (web.rs, lines 21-124)

**Parameters:** `url` (http/https), `extract_mode` ("markdown" or "text"), `max_chars` (100-200000)
**Limits:** Max 200KB output
**Security:** Goes through SSRF-safe `Fetcher` (blocks private IPs)
**Processing:** HTML → markdown/text conversion, title extraction

### MemorySearchTool (memory.rs, lines 168-226)

**Parameters:** `query` (string), `limit` (1-20)
**Depends on:** `ToolContext.MemorySearch` service
**Returns:** Ranked results with source, text, score

### MemoryGetTool (memory.rs, lines 22-120)

**Parameters:** `path` (relative to `workspace/memory/`), `from` (line offset), `lines` (max 5000)
**Limits:** Max 100KB output
**Security:** Path can't escape memory directory

### MessageSendTool (messaging.rs, lines 13-119)

**Parameters:** `channel`, `to`, `text`, `account_id` (optional), `thread_id` (optional)
**Limits:** Max 4000 chars per message
**Risk:** Mutating

### ImageDescribeTool (image.rs, lines 59-207)

**Parameters:** `paths` (array, max 10), `prompt` (optional)
**Limits:** Max 20MB per image
**Formats:** jpg, png, gif, webp, bmp, svg
**Output:** Returns `_image_content` array (base64) for vision models

### BrowserOpenTool (lib.rs, lines 1351-1382)

**Parameters:** `url` (http/https), `session_id` (optional)
**Infrastructure:** Chrome DevTools Protocol via `BrowserClient`
**Limits:** Max 10 concurrent sessions, 15s CDP command timeout
**Returns:** Page snapshot (title, URL, visible text)

## Approval Flow (in runtime, not tools crate)

When `ApprovalCh` is set in `ChatRequest` and tool risk > ReadOnly:

```
1. Runtime builds ApprovalRequest{tool_name, args, risk_level}
2. Sends to ApprovalCh
3. Waits for ApprovalDecision on response channel
4. AllowOnce → proceed
5. AllowAlways → proceed + cache for future calls
6. Deny → return error result to model ("Tool denied by user")
```

## How Tools Interact with the Agentic Loop

```
Runtime.chat()
    │
    ├── Phase 1: tools.Definitions(agent.AllowedTools)
    │   → Returns []ToolDef for system prompt (model knows what tools exist)
    │
    ├── Phase 3, each round:
    │   Model returns ToolCallResponse{id, name, arguments}
    │   │
    │   ├── toolTracker.Record(name, args) → loop detection
    │   ├── json.Unmarshal(arguments) → parse args
    │   ├── if name == "spawn_subagent" → handleSpawnSubagent()
    │   ├── approval check (if ApprovalCh set + risk > ReadOnly)
    │   ├── fireHook("tool.before")
    │   ├── tools.InvokeAllowed(allowed, name, args, toolCtx) → ToolOutput
    │   ├── fireHook("tool.after")
    │   ├── scanForLeaks(output) → credential detection
    │   ├── truncate(output, 400_000 chars)
    │   └── sessions.AppendTranscript(toolResult)
    │
    └── Feed tool results back as CompletionMessage(role=Tool) → next round
```

## Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `FRANKCLAW_TOOL_APPROVAL` | "readonly" | Auto-approval level |
| `FRANKCLAW_BASH_POLICY` | "deny-all" | Bash command policy |
| `FRANKCLAW_SANDBOX` | "none" | Sandbox mode for bash |
| `FRANKCLAW_ALLOW_BROWSER_MUTATIONS` | "0" | Legacy: "1" → Mutating approval |
| `FRANKCLAW_BROWSER_DEVTOOLS_URL` | "http://127.0.0.1:9222/" | Chrome CDP endpoint |
