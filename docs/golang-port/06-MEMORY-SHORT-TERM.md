# 06 - Short-Term Memory (Context Window)

## Overview

Short-term memory is the **conversation context** that fits within a single LLM call. It's ephemeral (RAM only), managed per-turn, and subject to token budgeting. When it exceeds the model's context window, oldest messages are pruned.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-runtime/src/context.rs` | 1-200 | Context window optimization, token estimation, pruning |
| `crates/frankclaw-runtime/src/lib.rs` | 430-468 | Context optimization call in chat loop |
| `crates/frankclaw-runtime/src/lib.rs` | 899-929 | Mid-loop re-compaction after tool results |
| `crates/frankclaw-runtime/prompts/context_compaction.md` | ~5 | Compaction marker template |
| `crates/frankclaw-core/src/model.rs` | 71-148 | `CompletionMessage` (the message unit) |
| `crates/frankclaw-core/src/sanitize.rs` | ~226 | `check_prompt_size()` (2MB hard cap) |

## What Constitutes Short-Term Memory

Short-term memory = the `Vec<CompletionMessage>` that gets sent to the LLM in a single `CompletionRequest`:

```go
type CompletionMessage struct {
    Role         Role               // System, User, Assistant, Tool
    Content      string             // Message text
    ToolCalls    []ToolCallResponse // Assistant→Tool invocations
    ToolCallID   *string            // Tool result reference
    ImageContent []ImageContent     // Vision content
}
```

This vector is built from:
1. **Transcript history** (loaded from session store, last 200 messages)
2. **Current user message** (appended at the end)
3. **Tool call/result pairs** (accumulated during the agentic loop)

## Token Estimation (context.rs:40-54)

Intentionally conservative (better to prune too early than overflow):

```go
const CharsPerToken = 3.5 // Conservative for English text

func EstimateTokens(text string) int {
    return int(math.Ceil(float64(len(text)) / CharsPerToken))
}

func EstimateMessagesTokens(messages []CompletionMessage) int {
    total := 0
    for _, msg := range messages {
        total += EstimateTokens(msg.Content) + 4 // 4 tokens per message overhead (role, delimiters)
    }
    return total
}
```

## Token Budget Calculation (context.rs:57-66)

```go
const SafetyMargin = 1.2       // Reserve 20% headroom
const SystemOverheadTokens = 2048 // System prompt frame overhead

func AvailableInputBudget(model ModelDef, systemPrompt string) int {
    reservedOutput := model.MaxOutputTokens
    if reservedOutput == 0 {
        reservedOutput = 4096
    }
    systemTokens := SystemOverheadTokens + EstimateTokens(systemPrompt)
    raw := model.ContextWindow - reservedOutput - systemTokens
    return int(float64(raw) / SafetyMargin)
}
```

**Example calculation:**
```
Model: Claude Sonnet 4 (200k context, 8192 max output)
System prompt: ~2000 chars = ~572 tokens

budget = (200000 - 8192 - (2048 + 572)) / 1.2
       = (200000 - 8192 - 2620) / 1.2
       = 189188 / 1.2
       = ~157656 tokens available for messages
```

## Context Optimization Algorithm (context.rs:77-139)

```go
func OptimizeContext(messages []CompletionMessage, model ModelDef, systemPrompt string) ContextWindow {
    // Step 1: Repair orphaned tool messages
    messages = RepairToolPairing(messages)

    // Step 2: Merge consecutive same-role messages (required by Anthropic/Gemini)
    messages = MergeConsecutiveSameRole(messages)

    // Step 3: Check if fits
    budget := AvailableInputBudget(model, systemPrompt)
    tokens := EstimateMessagesTokens(messages)
    if tokens <= budget {
        return ContextWindow{Messages: messages, EstimatedTokens: tokens}
    }

    // Step 4: Prune oldest messages (keep minimum 4)
    prunedCount := 0
    for len(messages) > MinKeepMessages && EstimateMessagesTokens(messages) > budget {
        messages = messages[1:]
        prunedCount++
    }

    // Step 5: Insert summary marker
    marker := CompletionMessage{
        Role:    RoleSystem,
        Content: fmt.Sprintf("[Previous conversation summary]\n(%d earlier messages were pruned to fit the context window.)", prunedCount),
    }
    messages = append([]CompletionMessage{marker}, messages...)

    return ContextWindow{
        Messages:        messages,
        EstimatedTokens: EstimateMessagesTokens(messages),
        PrunedCount:     prunedCount,
        Compacted:       true,
    }
}
```

### ContextWindow result struct:

```go
type ContextWindow struct {
    Messages        []CompletionMessage
    EstimatedTokens int
    PrunedCount     int
    Compacted       bool // true if messages were pruned
}
```

## Tool Message Repair (context.rs:146-175)

LLM APIs require tool results to follow assistant messages with tool_calls. After pruning, these pairs can become orphaned:

```go
func RepairToolPairing(messages []CompletionMessage) []CompletionMessage {
    var result []CompletionMessage
    for i, msg := range messages {
        // Remove Tool messages not preceded by Assistant with tool_calls
        if msg.Role == RoleTool {
            if i == 0 || !hasToolCalls(messages[i-1]) {
                continue // orphaned tool result, skip
            }
        }
        result = append(result, msg)
    }
    // Remove trailing Assistant with tool_calls but no following Tool results
    if len(result) > 0 && hasToolCalls(result[len(result)-1]) {
        result = result[:len(result)-1]
    }
    return result
}
```

## Consecutive Role Merging (context.rs:179-194)

Anthropic and Gemini APIs reject consecutive messages from the same role:

```go
func MergeConsecutiveSameRole(messages []CompletionMessage) []CompletionMessage {
    if len(messages) == 0 { return messages }
    var result []CompletionMessage
    result = append(result, messages[0])
    for i := 1; i < len(messages); i++ {
        if messages[i].Role == result[len(result)-1].Role &&
           len(messages[i].ToolCalls) == 0 && messages[i].ToolCallID == nil {
            result[len(result)-1].Content += "\n\n" + messages[i].Content
        } else {
            result = append(result, messages[i])
        }
    }
    return result
}
```

## Mid-Loop Re-Compaction (lib.rs:899-929)

After tool execution, the context may overflow again. The runtime re-compacts:

```go
// In the agentic loop, after all tool results are appended:
tokens := EstimateMessagesTokens(messages)
if tokens > budget {
    ctx := OptimizeContext(messages, modelDef, systemPrompt)
    messages = ctx.Messages
    if EstimateMessagesTokens(messages) > budget {
        return nil, errors.New("context still over budget after compaction")
    }
}
```

## Hard Prompt Size Limit (sanitize.rs)

Independent of token estimation, there's a hard byte limit:

```go
const MaxPromptBytes = 2 * 1024 * 1024 // 2 MB

func CheckPromptSize(messages []CompletionMessage, system string) bool {
    total := len(system)
    for _, msg := range messages {
        total += len(msg.Content)
    }
    return total <= MaxPromptBytes
}
```

This prevents token exhaustion DoS regardless of model context window.

## When Short-Term Memory is Used in the Loop

```
1. Load transcript (200 msgs) → convert to []CompletionMessage
2. Append user message
3. OptimizeContext() → fit to budget        ← FIRST COMPACTION
4. Send to model
5. Model returns tool_calls
6. Execute tools → append tool results
7. EstimateTokens > budget? → OptimizeContext() ← MID-LOOP COMPACTION
8. Send to model again (round 2)
9. ... repeat up to 5 rounds
```

## Constants Summary

| Constant | Value | Location |
|---|---|---|
| `CHARS_PER_TOKEN` | 3.5 | context.rs |
| `SAFETY_MARGIN` | 1.2 | context.rs |
| `MIN_KEEP_MESSAGES` | 4 | context.rs |
| `SYSTEM_OVERHEAD_TOKENS` | 2048 | context.rs |
| `MAX_PROMPT_BYTES` | 2 MB | sanitize.rs |
| `MAX_TOOL_RESULT_CHARS` | 400,000 | lib.rs |

## Go Implementation Notes

1. Token estimation is a rough heuristic - don't over-engineer it
2. The 3.5 chars/token ratio is conservative for English; for CJK text it's ~1.5-2
3. Mid-loop compaction is critical - tool chains can add 400KB per tool call
4. RepairToolPairing must run before sending to ANY provider API
5. MergeConsecutiveSameRole is required by Anthropic and Gemini, but OpenAI tolerates it
6. The `[Previous conversation summary]` marker is a system message, not a user message
