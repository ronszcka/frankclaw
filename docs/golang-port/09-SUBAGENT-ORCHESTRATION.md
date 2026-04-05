# 09 - Subagent Orchestration

## Overview

The subagent system allows the LLM to spawn child agents for complex subtasks. It's a **recursive** architecture: the parent's agentic loop calls `Runtime::chat()` again with a new session, and the child can spawn grandchildren up to a configurable depth limit.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-runtime/src/subagent.rs` | 1-381 | `SubagentRegistry`, `SpawnRequest`, `RunRecord`, `CompletionNotice` |
| `crates/frankclaw-runtime/src/lib.rs` | 325-329 | Spawn tool injection (depth check) |
| `crates/frankclaw-runtime/src/lib.rs` | 651-688 | Special handling for spawn_subagent tool |
| `crates/frankclaw-runtime/src/lib.rs` | 1176-1281 | `handle_spawn_subagent()` - recursive execution |
| `crates/frankclaw-runtime/src/lib.rs` | 1788-1812 | `spawn_subagent` tool definition |
| `crates/frankclaw-runtime/src/prompts.rs` | 18-22 | Subagent prompt constants |
| `crates/frankclaw-runtime/prompts/subagent_identity.md` | ~1 | Identity template |
| `crates/frankclaw-runtime/prompts/subagent_timeout.md` | ~1 | Timeout template |
| `crates/frankclaw-runtime/prompts/subagent_can_spawn.md` | ~1 | Can-spawn hint |
| `crates/frankclaw-runtime/prompts/subagent_max_depth.md` | ~1 | Max-depth hint |

## Constants

| Name | Value | Purpose |
|---|---|---|
| `DEFAULT_MAX_DEPTH` | 3 | Max hierarchy depth (parent=0, child=1, grandchild=2, great-grandchild=3) |
| `DEFAULT_MAX_CHILDREN` | 5 | Max concurrent children per parent |
| `DEFAULT_TIMEOUT_SECS` | 300 | 5 minutes per subagent |

## Core Types (subagent.rs)

### SpawnRequest (lines 48-63)

```go
type SpawnRequest struct {
    Task             string     // What the subagent should do
    Label            *string    // Human-readable label (optional)
    AgentID          AgentID    // Agent to run as
    ModelOverride    *string    // Optional model override
    TimeoutSecs      *uint64    // Execution timeout
    ParentSessionKey SessionKey // Parent's session context
    ParentDepth      uint32     // Parent's depth in hierarchy
}
```

### RunRecord (lines 99-115)

```go
type RunRecord struct {
    RunID            RunID
    ChildSessionKey  SessionKey
    ParentSessionKey SessionKey
    AgentID          AgentID
    Task             string
    Label            *string
    ModelOverride    *string
    Depth            uint32    // 0=top-level, 1=child, 2=grandchild
    State            RunState
    CreatedAt        time.Time
    StartedAt        *time.Time
    EndedAt          *time.Time
    ResultText       *string
    Error            *string
    TimeoutSecs      uint64
}
```

### RunState (lines 82-95)

```go
type RunState string
const (
    RunStatePending   RunState = "pending"
    RunStateRunning   RunState = "running"
    RunStateCompleted RunState = "completed"
    RunStateFailed    RunState = "failed"
    RunStateTimedOut  RunState = "timed_out"
    RunStateKilled    RunState = "killed"
)
```

### CompletionNotice (lines 118-124)

```go
type CompletionNotice struct {
    RunID      RunID
    State      RunState
    ResultText *string
    Error      *string
}
```

## SubagentRegistry (subagent.rs:127-328)

Central registry for tracking all subagent executions:

```go
type SubagentRegistry struct {
    mu          sync.Mutex
    runs        map[RunID]*RunRecord
    completions map[RunID]chan CompletionNotice // one-shot delivery
    maxDepth    uint32
    maxChildren int
}

func NewSubagentRegistry() *SubagentRegistry {
    return &SubagentRegistry{
        runs:        make(map[RunID]*RunRecord),
        completions: make(map[RunID]chan CompletionNotice),
        maxDepth:    3,
        maxChildren: 5,
    }
}
```

### Key Methods

#### RegisterSpawn (lines 160-224)

```go
type SpawnResult struct {
    Accepted bool
    RunID    RunID
    ChildKey SessionKey
    Reason   string // if rejected
}

func (r *SubagentRegistry) RegisterSpawn(req *SpawnRequest) SpawnResult {
    r.mu.Lock()
    defer r.mu.Unlock()

    childDepth := req.ParentDepth + 1

    // Check depth limit
    if childDepth > r.maxDepth {
        return SpawnResult{Reason: fmt.Sprintf("max depth %d exceeded", r.maxDepth)}
    }

    // Count active children of parent
    activeChildren := 0
    for _, run := range r.runs {
        if run.ParentSessionKey == req.ParentSessionKey &&
           (run.State == RunStatePending || run.State == RunStateRunning) {
            activeChildren++
        }
    }
    if activeChildren >= r.maxChildren {
        return SpawnResult{Reason: fmt.Sprintf("max children %d reached", r.maxChildren)}
    }

    // Create run record
    runID := NewRunID() // UUID
    childKey := SessionKey(fmt.Sprintf("subagent:%s:%s", req.AgentID, runID))

    record := &RunRecord{
        RunID:            runID,
        ChildSessionKey:  childKey,
        ParentSessionKey: req.ParentSessionKey,
        AgentID:          req.AgentID,
        Task:             req.Task,
        Label:            req.Label,
        Depth:            childDepth,
        State:            RunStatePending,
        CreatedAt:        time.Now(),
        TimeoutSecs:      300, // default
    }
    if req.TimeoutSecs != nil {
        record.TimeoutSecs = *req.TimeoutSecs
    }

    r.runs[runID] = record
    return SpawnResult{Accepted: true, RunID: runID, ChildKey: childKey}
}
```

#### MarkRunning, Complete, ListChildren, Kill, etc.

```go
func (r *SubagentRegistry) MarkRunning(runID RunID) error
func (r *SubagentRegistry) Complete(notice CompletionNotice) error
func (r *SubagentRegistry) ListenCompletion(runID RunID) <-chan CompletionNotice
func (r *SubagentRegistry) DepthOf(sessionKey SessionKey) uint32
func (r *SubagentRegistry) ListChildren(parentKey SessionKey) []RunRecord
func (r *SubagentRegistry) ActiveRuns() []RunRecord
func (r *SubagentRegistry) Kill(runID RunID) error
func (r *SubagentRegistry) CleanupOldRuns(maxAge time.Duration)
```

## Spawn Tool Definition (lib.rs:1788-1812)

The `spawn_subagent` tool is injected into the agent's tool list if depth < max:

```go
func SpawnSubagentToolDef() ToolDef {
    return ToolDef{
        Name:        "spawn_subagent",
        Description: "Spawn a subagent to handle a complex subtask.",
        Parameters: json.RawMessage(`{
            "type": "object",
            "properties": {
                "task": {"type": "string", "description": "Detailed description of the subtask"},
                "label": {"type": "string", "description": "Short label for tracking"}
            },
            "required": ["task"]
        }`),
        RiskLevel: Mutating,
    }
}
```

### Injection Logic (lib.rs:325-329)

```go
// In Runtime.Chat(), before sending tools to model:
currentDepth := r.subagentRegistry.DepthOf(sessionKey)
if currentDepth < DEFAULT_MAX_DEPTH {
    allowedTools = append(allowedTools, SpawnSubagentToolDef())
}
```

## Spawn Handler (lib.rs:1176-1281)

When the model calls `spawn_subagent`:

```go
func (r *Runtime) handleSpawnSubagent(ctx context.Context, args map[string]interface{}, parentSession SessionKey, parentDepth uint32) (string, error) {
    // 1. Extract and sanitize task/label
    task := sanitizeForPrompt(args["task"].(string))
    var label *string
    if l, ok := args["label"]; ok {
        s := sanitizeForPrompt(l.(string))
        label = &s
    }

    // 2. Register spawn
    result := r.subagentRegistry.RegisterSpawn(&SpawnRequest{
        Task:             task,
        Label:            label,
        AgentID:          r.defaultAgent(), // same agent as parent
        ParentSessionKey: parentSession,
        ParentDepth:      parentDepth,
    })
    if !result.Accepted {
        return "", fmt.Errorf("spawn rejected: %s", result.Reason)
    }

    // 3. Mark running
    r.subagentRegistry.MarkRunning(result.RunID)

    // 4. Recursive call with timeout
    timeout := time.Duration(300) * time.Second
    childCtx, cancel := context.WithTimeout(ctx, timeout)
    defer cancel()

    response, err := r.Chat(childCtx, ChatRequest{
        SessionKey: &result.ChildKey,
        Message:    task,
        // NO streaming (subagent output goes back to parent, not user)
        // NO approval channel (subagent can't ask user for approval)
    })

    // 5. Report completion
    if err != nil {
        if errors.Is(err, context.DeadlineExceeded) {
            r.subagentRegistry.Complete(CompletionNotice{
                RunID: result.RunID,
                State: RunStateTimedOut,
                Error: strPtr("subagent timed out"),
            })
            return "Subagent timed out after 300 seconds", nil
        }
        r.subagentRegistry.Complete(CompletionNotice{
            RunID: result.RunID,
            State: RunStateFailed,
            Error: strPtr(err.Error()),
        })
        return fmt.Sprintf("Subagent failed: %s", err), nil
    }

    r.subagentRegistry.Complete(CompletionNotice{
        RunID:      result.RunID,
        State:      RunStateCompleted,
        ResultText: &response.Content,
    })

    return response.Content, nil
}
```

## Subagent Context Building (subagent.rs:343-381)

System prompt additions for subagents:

```go
func BuildSubagentContext(record *RunRecord, maxDepth uint32) string {
    var parts []string

    // Identity
    parts = append(parts, fmt.Sprintf(
        "You are a subagent (depth %d/%d) spawned to complete a specific task.",
        record.Depth, maxDepth,
    ))

    // Task label
    if record.Label != nil {
        label := truncate(*record.Label, 2000)
        parts = append(parts, "Task label: "+label)
    }

    // Task description
    task := truncate(record.Task, 2000)
    parts = append(parts, "Task: "+task)

    // Timeout
    parts = append(parts, fmt.Sprintf(
        "Timeout: %d seconds. Complete the task and report your findings concisely.",
        record.TimeoutSecs,
    ))

    // Spawn permission
    if record.Depth < maxDepth {
        parts = append(parts, "You may spawn sub-subagents if needed, but prefer completing the task directly.")
    } else {
        parts = append(parts, "You are at maximum spawn depth and cannot spawn further subagents.")
    }

    return strings.Join(parts, "\n\n")
}
```

## Execution Flow Diagram

```
Parent Agent (depth=0) calls: spawn_subagent(task="Find all TODOs")
    │
    ├── sanitize task + label
    ├── registry.RegisterSpawn() → runID, childSessionKey
    │   ├── Check depth limit (0+1=1 ≤ 3) ✓
    │   └── Check children count (<5) ✓
    │
    ├── registry.MarkRunning(runID)
    │
    ├── runtime.Chat(childSessionKey, message=task)  ← RECURSIVE
    │   │
    │   ├── depth_of(childSessionKey) = 1
    │   ├── System prompt includes: "You are a subagent (depth 1/3)"
    │   ├── spawn_subagent tool IS available (depth 1 < 3)
    │   │
    │   ├── [Child's own agentic loop runs]
    │   │   ├── Can call tools
    │   │   ├── Can spawn grandchildren (depth 2)
    │   │   └── Stops when done or timeout
    │   │
    │   └── Returns ChatResponse
    │
    ├── registry.Complete(CompletionNotice{Completed, result})
    │
    └── Returns result to parent as tool output
        │
        └── Parent continues with result in context
```

## Session Key Format

- **Regular sessions:** `{agent_id}:{channel}:{account_id}`
- **Subagent sessions:** `subagent:{agent_id}:{run_uuid}`

This allows detecting subagent sessions by prefix: `strings.HasPrefix(key, "subagent:")`

## Safety Mechanisms

| Mechanism | Protection |
|---|---|
| Depth limit (3) | Prevents infinite recursion |
| Children limit (5) | Prevents fork bombs |
| Timeout (300s) | Prevents runaway execution |
| Task sanitization | Strips Unicode control chars (prompt injection prevention) |
| Task truncation (2000 chars) | Prevents memory exhaustion |
| No approval channel | Subagents can't prompt user (would block parent) |
| No streaming | Subagent output goes to parent, not directly to user |

## Go Implementation Notes

1. **Recursive `Chat()` call** is the core mechanism - no separate subagent runtime needed
2. **Use `context.WithTimeout()`** for subagent timeout instead of tokio's `time::timeout()`
3. **Use `chan CompletionNotice` (cap 1)** for one-shot completion notification
4. **Session key with UUID** ensures unique session per subagent run
5. **Registry cleanup:** Periodically call `CleanupOldRuns()` to prevent memory leaks
6. **Max 15 active subagents** per parent path (3 depth × 5 children) - reasonable limit
