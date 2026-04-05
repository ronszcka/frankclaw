# 12 - Hooks & Event System

## Overview

The hook system provides lifecycle event notifications throughout the agentic loop. Hooks fire asynchronously (fire-and-forget) and allow external systems to observe or react to events like message receipt, tool invocation, session creation, etc.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-core/src/hooks.rs` | 1-502 | `HookRegistry`, `HookEvent`, `EventType`, handler registration |
| `crates/frankclaw-runtime/src/lib.rs` | 232-239 | `fire_hook()` helper in Runtime |
| `crates/frankclaw-runtime/src/lib.rs` | 316 | `message.received` hook |
| `crates/frankclaw-runtime/src/lib.rs` | 555 | `message.sent` hook |
| `crates/frankclaw-runtime/src/lib.rs` | 747 | `tool.before` hook |
| `crates/frankclaw-runtime/src/lib.rs` | 777, 787 | `tool.after` hook (success/failure) |

## EventType (hooks.rs:18-26)

```go
type EventType string
const (
    EventCommand  EventType = "command"
    EventSession  EventType = "session"
    EventAgent    EventType = "agent"
    EventGateway  EventType = "gateway"
    EventMessage  EventType = "message"
    EventTool     EventType = "tool"
)
```

## HookEvent (hooks.rs:42-74)

```go
type HookEvent struct {
    EventType EventType              `json:"event_type"`
    Action    string                 `json:"action"`   // "received", "sent", "before", "after", etc.
    Context   map[string]interface{} `json:"context"`  // arbitrary key-value metadata
}

// Constructor
func NewHookEvent(eventType EventType, action string) *HookEvent {
    return &HookEvent{
        EventType: eventType,
        Action:    action,
        Context:   make(map[string]interface{}),
    }
}

// Builder pattern
func (e *HookEvent) With(key string, value interface{}) *HookEvent {
    e.Context[key] = value
    return e
}

// Specific key for handler matching: "message:received"
func (e *HookEvent) SpecificKey() string {
    return string(e.EventType) + ":" + e.Action
}
```

### Convenience Constructors (hooks.rs)

```go
func MessageReceivedEvent(sessionKey, agentID, channel, text string) *HookEvent {
    return NewHookEvent(EventMessage, "received").
        With("session_key", sessionKey).
        With("agent_id", agentID).
        With("channel", channel).
        With("text", text)
}

func MessageSentEvent(sessionKey, agentID, content string) *HookEvent {
    return NewHookEvent(EventMessage, "sent").
        With("session_key", sessionKey).
        With("agent_id", agentID).
        With("content", content)
}

func ToolBeforeEvent(sessionKey, toolName, toolArgs string) *HookEvent {
    return NewHookEvent(EventTool, "before").
        With("session_key", sessionKey).
        With("tool_name", toolName).
        With("tool_args", toolArgs)
}

func ToolAfterEvent(sessionKey, toolName string, success bool, durationMs int64) *HookEvent {
    return NewHookEvent(EventTool, "after").
        With("session_key", sessionKey).
        With("tool_name", toolName).
        With("success", success).
        With("duration_ms", durationMs)
}

func SessionCreatedEvent(sessionKey, agentID, channel string) *HookEvent {
    return NewHookEvent(EventSession, "created").
        With("session_key", sessionKey).
        With("agent_id", agentID).
        With("channel", channel)
}

func SessionResetEvent(sessionKey, reason string) *HookEvent {
    return NewHookEvent(EventSession, "reset").
        With("session_key", sessionKey).
        With("reason", reason)
}

func GatewayStartedEvent(port int, authMode string) *HookEvent {
    return NewHookEvent(EventGateway, "started").
        With("port", port).
        With("auth_mode", authMode)
}

func AgentTurnStartedEvent(sessionKey, agentID, modelID string) *HookEvent {
    return NewHookEvent(EventAgent, "turn_started").
        With("session_key", sessionKey).
        With("agent_id", agentID).
        With("model_id", modelID)
}

func AgentTurnCompletedEvent(sessionKey string, toolsUsed []string, usage Usage) *HookEvent {
    return NewHookEvent(EventAgent, "turn_completed").
        With("session_key", sessionKey).
        With("tools_used", toolsUsed).
        With("usage", usage)
}

func CommandExecutedEvent(command, output string) *HookEvent {
    return NewHookEvent(EventCommand, "executed").
        With("command", command).
        With("output", output)
}
```

## HookRegistry (hooks.rs:91-216)

```go
type HookHandler func(ctx context.Context, event *HookEvent) error

type handlerEntry struct {
    Name    string
    Handler HookHandler
}

type HookRegistry struct {
    mu       sync.RWMutex
    handlers map[string][]handlerEntry // key: "message" or "message:received"
}

func NewHookRegistry() *HookRegistry {
    return &HookRegistry{
        handlers: make(map[string][]handlerEntry),
    }
}
```

### Registration

```go
// Register for ALL events of a type (e.g., all "message" events)
func (r *HookRegistry) On(eventType EventType, name string, handler HookHandler) {
    r.mu.Lock()
    defer r.mu.Unlock()
    key := string(eventType)
    r.handlers[key] = append(r.handlers[key], handlerEntry{Name: name, Handler: handler})
}

// Register for SPECIFIC event (e.g., "message:received" only)
func (r *HookRegistry) OnAction(eventType EventType, action, name string, handler HookHandler) {
    r.mu.Lock()
    defer r.mu.Unlock()
    key := string(eventType) + ":" + action
    r.handlers[key] = append(r.handlers[key], handlerEntry{Name: name, Handler: handler})
}
```

### Firing (hooks.rs:136-184)

```go
const hookTimeout = 30 * time.Second

func (r *HookRegistry) Fire(event *HookEvent) {
    r.mu.RLock()

    // Collect all matching handlers
    var handlers []handlerEntry

    // General type handlers (e.g., all "message" handlers)
    if h, ok := r.handlers[string(event.EventType)]; ok {
        handlers = append(handlers, h...)
    }

    // Specific type:action handlers (e.g., "message:received")
    specificKey := event.SpecificKey()
    if h, ok := r.handlers[specificKey]; ok {
        handlers = append(handlers, h...)
    }

    r.mu.RUnlock()

    // Run ALL handlers in parallel, fire-and-forget
    for _, h := range handlers {
        go func(entry handlerEntry) {
            ctx, cancel := context.WithTimeout(context.Background(), hookTimeout)
            defer cancel()

            if err := entry.Handler(ctx, event); err != nil {
                log.Warn("hook handler failed",
                    "name", entry.Name,
                    "event", event.SpecificKey(),
                    "error", err)
            }
        }(h)
    }
}
```

### Clear Handlers

```go
func (r *HookRegistry) Clear(eventType EventType) {
    r.mu.Lock()
    defer r.mu.Unlock()
    delete(r.handlers, string(eventType))
    // Also clear specific keys
    for key := range r.handlers {
        if strings.HasPrefix(key, string(eventType)+":") {
            delete(r.handlers, key)
        }
    }
}

func (r *HookRegistry) HandlerCount() int {
    r.mu.RLock()
    defer r.mu.RUnlock()
    count := 0
    for _, handlers := range r.handlers {
        count += len(handlers)
    }
    return count
}
```

## Integration in Runtime (lib.rs:232-239)

```go
// Helper in Runtime struct
func (r *Runtime) fireHook(event *HookEvent) {
    if r.hooks != nil {
        go r.hooks.Fire(event) // fire-and-forget, never blocks the loop
    }
}
```

### Hook Points in Agentic Loop

```
Runtime.Chat()
    │
    ├── fireHook(MessageReceivedEvent(...))     ← line 316
    │
    ├── [AGENTIC LOOP]
    │   │
    │   ├── For each tool call:
    │   │   ├── fireHook(ToolBeforeEvent(...))  ← line 747
    │   │   ├── [execute tool]
    │   │   └── fireHook(ToolAfterEvent(...))   ← line 777/787
    │   │
    │   └── [loop continues...]
    │
    └── fireHook(MessageSentEvent(...))         ← line 555
```

## Event Lifecycle

```
message.received     → User message arrives
agent.turn_started   → Before first LLM call
tool.before          → Before each tool invocation
tool.after           → After each tool invocation (with success/failure + duration)
agent.turn_completed → After final response (with usage stats)
message.sent         → Response delivered to user
session.created      → New session created
session.reset        → Session transcript cleared
gateway.started      → Gateway server started
command.executed     → CLI command executed
```

## Use Cases for Hooks

| Hook | Use Case |
|---|---|
| `message.received` | Logging, analytics, rate limiting |
| `message.sent` | Logging, analytics, notification |
| `tool.before` | Audit trail, permission checks |
| `tool.after` | Performance monitoring, error tracking |
| `session.created` | Welcome messages, onboarding |
| `session.reset` | Cleanup, archival |
| `gateway.started` | Health check registration |
| `agent.turn_completed` | Cost tracking, usage reporting |

## Go Implementation Notes

1. **Fire-and-forget:** Use goroutines for each handler - never block the agentic loop
2. **30s timeout:** Use `context.WithTimeout()` per handler invocation
3. **RWMutex:** Read lock for firing (frequent), write lock for registration (rare)
4. **No return values:** Hooks are observational, they don't modify the flow
5. **Error handling:** Log errors, never propagate (hooks must not break the loop)
6. **Handler naming:** Names are for debugging/logging, not for deduplication
7. **Thread safety:** The handler slice is copied before releasing the read lock to avoid holding it during execution
