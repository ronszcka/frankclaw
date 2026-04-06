# 03 - Agentic Loop (The Brain)

## Overview

The agentic loop is the core reasoning engine. It receives a user message, orchestrates LLM calls, executes tool calls, and loops until the LLM produces a final response. This is **the most important feature to port**.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-runtime/src/lib.rs` | 300-935 | `Runtime::chat()` - main loop |
| `crates/frankclaw-runtime/src/context.rs` | 1-200 | Context window optimization |
| `crates/frankclaw-runtime/src/prompts.rs` | 1-100 | Prompt template rendering |
| `crates/frankclaw-runtime/src/sanitize.rs` | 1-6 | Re-exports from core |
| `crates/frankclaw-runtime/src/leak_detector.rs` | 1-150+ | Credential leak scanning |
| `crates/frankclaw-runtime/prompts/*.md` | ~10 files | Prompt templates |
| `crates/frankclaw-core/src/model.rs` | 70-270 | `CompletionMessage`, `CompletionRequest/Response` |
| `crates/frankclaw-core/src/session.rs` | 131-161 | `SessionStore` trait |
| `crates/frankclaw-core/src/sanitize.rs` | 1-226 | `sanitize_for_prompt()`, `check_prompt_size()` |

## Entry Point: `Runtime::chat()`

**File:** `crates/frankclaw-runtime/src/lib.rs`, lines 300-935

```go
// Go equivalent signature
func (r *Runtime) Chat(ctx context.Context, req ChatRequest) (*ChatResponse, error)
```

### ChatRequest (lines 53-74)

```go
type ChatRequest struct {
    AgentID             *AgentID
    SessionKey          *SessionKey
    Message             string
    Attachments         []Attachment
    ModelID             *string
    MaxTokens           *int
    Temperature         *float64
    StreamCh            chan<- StreamDelta     // nil = no streaming
    ThinkingBudget      *int
    ChannelID           *ChannelID
    ChannelCapabilities *ChannelCapabilities
    Canvas              CanvasService          // optional
    CancelCtx           context.Context        // for cancellation
    ApprovalCh          chan<- ApprovalExchange // optional interactive approval
}
```

### ChatResponse

```go
type ChatResponse struct {
    Content    string
    Usage      Usage
    ToolsUsed  []string
    SessionKey SessionKey
}
```

## The Loop - Phase by Phase

### Phase 1: Initialization (lines 300-435)

```
1. Sanitize user message          → sanitize_for_prompt(message)
2. Resolve agent                  → find agent config by ID or use default
3. Resolve model                  → request model → agent model → global default
4. Load/create session            → sessions.Get(key) or sessions.Upsert(new)
5. Fire "message.received" hook   → async, fire-and-forget
6. Load transcript history        → sessions.GetTranscript(key, limit=200)
7. Get allowed tools              → tools.Definitions(agent.AllowedTools)
8. Inject spawn_subagent tool     → if depth < MAX_DEPTH
9. Look up model definition       → catalog.Find(model_id) for context_window, capabilities
10. Process media attachments     → if vision: encode images; else: run understanding
11. Build CompletionMessages      → convert transcript + append user message
```

**Key decisions:**
- Model selection follows a fallback chain: explicit request → agent config → global default → first available
- Transcript loads last 200 messages (configurable)
- If model doesn't support vision but images attached: runs through media understanding pipeline to get text descriptions

### Phase 2: Context Optimization (lines 437-468)

**File:** `crates/frankclaw-runtime/src/context.rs`

```go
func OptimizeContext(messages []CompletionMessage, modelDef ModelDef, systemPrompt string) ContextWindow
```

**Algorithm:**

```
1. Build system prompt (see below)
2. Calculate token budget:
   budget = (context_window - reserved_output - system_overhead) / SAFETY_MARGIN
   where:
     reserved_output = model.MaxOutputTokens or 4096
     system_overhead = 2048 + estimate_tokens(system_prompt)
     SAFETY_MARGIN = 1.2 (reserves 20% headroom)

3. Estimate current message tokens:
   tokens = sum(len(msg.Content) / 3.5 + 4) for each message

4. If fits → return as-is

5. If over budget:
   a. Repair orphaned tool messages (remove tool results without preceding call)
   b. Merge consecutive same-role messages
   c. Prune oldest messages (keep minimum 4)
   d. Insert "[Previous conversation summary]" marker
   e. Return optimized context
```

**Constants:**
| Name | Value | Purpose |
|---|---|---|
| `SAFETY_MARGIN` | 1.2 | 20% headroom for response |
| `MIN_KEEP_MESSAGES` | 4 | Never prune below this |
| `SYSTEM_OVERHEAD_TOKENS` | 2048 | Frame overhead |
| `CHARS_PER_TOKEN` | 3.5 | Conservative token estimation |

### System Prompt Assembly (lines 1299-1424)

Built in sections, concatenated:

```
Section 1: Agent identity
  "You are {agent_name}."

Section 2: User-defined system prompt (highest priority)
  From agent config: agent.system_prompt

Section 2b: Workspace bootstrap files
  Contents of SOUL.md, IDENTITY.md if present

Section 3: Available tools
  "You have access to the following tools:\n"
  For each tool: "- {name}: {description}\n  Parameters: {json_schema}"

Section 4: Skill prompts (if registered)

Section 5: Safety guidelines
  Static text about safety boundaries

Section 6: Runtime context
  "Agent: {agent_id}, Model: {model_id}, Date: {today}, Tools: {count}"

Section 7: Channel capabilities
  "Channel supports: threads, reactions, attachments" etc.

Section 8: Language instruction (non-English)
  "Please respond in {language}" when FRANKCLAW_LANG != "en"
```

### Phase 3: Main Reasoning Loop (lines 496-930)

```go
const MAX_TOOL_ROUNDS = 4  // max 5 iterations (0..=4)
const MAX_TOOL_CALLS_PER_TURN = 8
const MAX_TOOL_RESULT_CHARS = 400_000
const TURN_SAFETY_TIMEOUT = 600 * time.Second
```

**Loop structure:**

```
for round := 0; round <= MAX_TOOL_ROUNDS; round++ {
    // A. Check cancellation + timeout
    if ctx.Err() != nil { return ErrTurnCancelled }
    if time.Since(turnStart) > TURN_SAFETY_TIMEOUT { return ErrTimeout }

    // B. Call LLM provider
    response, err := failoverChain.Complete(ctx, request, streamCh)

    // C. No tool calls? → Done!
    if len(response.ToolCalls) == 0 {
        sessions.AppendTranscript(key, assistantEntry)
        fireHook("message.sent", ...)
        return &ChatResponse{Content: response.Content, Usage: response.Usage}
    }

    // D. Stream intermediate text (user sees progress)
    if response.Content != "" && streamCh != nil {
        streamCh <- StreamDelta{Type: "text", Text: response.Content}
    }

    // E. Append assistant message with tool_calls to transcript
    sessions.AppendTranscript(key, assistantWithToolCalls)
    messages = append(messages, assistantToolCallsMessage)

    // F. Execute each tool call
    for _, toolCall := range response.ToolCalls {
        // F1. Loop detection
        toolTracker.Record(toolCall.Name, toolCall.Arguments)
        // Warns at 3 repeats, blocks at 6

        // F2. Parse arguments
        args, err := json.Unmarshal(toolCall.Arguments)

        // F3. Special: subagent spawn
        if toolCall.Name == "spawn_subagent" {
            result = handleSpawnSubagent(ctx, args)
            goto appendResult
        }

        // F4. Interactive approval (if mutating/destructive)
        if approvalCh != nil && riskLevel > ReadOnly {
            decision := requestApproval(toolCall)
            if decision == Deny { result = "Tool denied by user"; goto appendResult }
        }

        // F5. Invoke tool
        fireHook("tool.before", toolCall)
        output, err := toolRegistry.InvokeAllowed(allowedTools, toolCall.Name, args, toolCtx)
        fireHook("tool.after", toolCall, err == nil)

        // F6. Credential leak detection
        leakResult := scanForLeaks(output)
        if leakResult.ShouldBlock { return ErrLeakDetected }

        // F7. Truncate + store result
        result = truncate(output, MAX_TOOL_RESULT_CHARS)

    appendResult:
        sessions.AppendTranscript(key, toolResultEntry)
        messages = append(messages, toolResultMessage)
    }

    // G. Re-compact context if over budget
    if estimateTokens(messages) > budget {
        messages = OptimizeContext(messages, modelDef, systemPrompt).Messages
    }
}

return ErrToolRoundLimitExceeded
```

## Tool Loop Tracker (lines 112-120)

Prevents infinite loops when model keeps calling the same tool:

```go
type ToolTracker struct {
    calls map[string]int // key: "toolName:sha256(args)"
}

const TOOL_REPEAT_WARN_THRESHOLD  = 3
const TOOL_REPEAT_BLOCK_THRESHOLD = 6

func (t *ToolTracker) Record(name, args string) error {
    key := name + ":" + sha256(args)
    t.calls[key]++
    if t.calls[key] >= TOOL_REPEAT_BLOCK_THRESHOLD {
        return ErrToolLoopDetected
    }
    if t.calls[key] >= TOOL_REPEAT_WARN_THRESHOLD {
        log.Warn("repeated tool call detected", "tool", name, "count", t.calls[key])
    }
    return nil
}
```

## Runtime Struct

```go
type Runtime struct {
    sessions         SessionStore
    tools            *ToolRegistry
    models           *FailoverChain
    hooks            *HookRegistry
    subagentRegistry *SubagentRegistry
    config           atomic.Value // *FrankClawConfig, hot-reloadable
    catalog          *ModelCatalog
    memory           MemorySearch        // optional
    fetcher          Fetcher             // optional
    channels         MessageSender       // optional
    cron             CronManager         // optional
    audioTranscriber AudioTranscriber    // optional
    canvas           CanvasService       // optional
}
```

## Flow Diagram

```
┌──────────────────────────────────────────────────┐
│ PHASE 1: INITIALIZATION                          │
│ Sanitize → Resolve agent/model/session           │
│ Load transcript (200 msgs) → Build system prompt │
│ Process attachments → Optimize context           │
└──────────────────┬───────────────────────────────┘
                   │
                   v
┌──────────────────────────────────────────────────┐
│ PHASE 3: AGENTIC LOOP (max 5 rounds)            │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │ Call LLM Provider (failover chain)         │  │
│  │ → CompletionResponse                       │  │
│  └─────────────┬──────────────────────────────┘  │
│                │                                  │
│          tool_calls?                              │
│          /        \                               │
│        NO         YES                             │
│        │           │                              │
│   [RETURN]    For each tool:                     │
│   Final       1. Loop detection                  │
│   response    2. Parse args                      │
│               3. Approval check (if mutating)    │
│               4. Execute tool                    │
│               5. Leak detection                  │
│               6. Truncate + persist result       │
│                    │                              │
│               Re-compact context if needed        │
│                    │                              │
│               [NEXT ROUND] ─────────────────>    │
│                                                  │
│ If MAX_ROUNDS exceeded → error                   │
└──────────────────────────────────────────────────┘
```

## Interactions

- **`context.rs`** is called at initialization AND mid-loop (re-compaction after tool results)
- **`prompts.rs`** is called once per turn to build the system prompt
- **`leak_detector.rs`** is called after EVERY tool execution
- **`SessionStore`** is written to after EVERY message (user, assistant, tool results)
- **`HookRegistry`** fires events at: message.received, tool.before, tool.after, message.sent
- **`SubagentRegistry`** handles recursive `spawn_subagent` tool calls
- **`FailoverChain`** handles model selection, retry, circuit breaking

## Go Implementation Notes

1. Use `context.Context` for cancellation instead of `CancellationToken`
2. Use `chan StreamDelta` for streaming instead of `mpsc::Sender<StreamDelta>`
3. Use `chan ApprovalExchange` for approval flow
4. The tool tracker can be a simple `map[string]int` (not concurrent, single goroutine owns the loop)
5. Token estimation: `len(content) / 3.5` is intentionally conservative
6. Mid-loop re-compaction is critical - without it, long tool chains overflow context
