# 11 - Smart Routing & Cost Management

## Overview

Smart routing analyzes message complexity to choose the cheapest model that can handle it. Cost management enforces daily budgets and hourly rate limits. Together they prevent overspending while maintaining quality.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-models/src/routing.rs` | 1-898 | 13-dimension complexity scorer + classification |
| `crates/frankclaw-models/src/cost_guard.rs` | 1-407 | Daily budget + hourly rate enforcement |
| `crates/frankclaw-models/src/costs.rs` | 1-154 | Per-model cost lookup table |
| `crates/frankclaw-models/src/cache.rs` | 1-362+ | Response cache (TTL + LRU) |
| `crates/frankclaw-models/src/catalog.rs` | 1-400+ | Static model metadata database |

## Smart Routing (routing.rs)

### Complexity Tiers

| Tier | Score Range | Use Case | Model Class |
|---|---|---|---|
| **Flash** | 0-15 | Simple greetings, quick lookups | Cheapest (gpt-4o-mini, haiku) |
| **Standard** | 16-40 | Writing, comparisons | Mid-tier |
| **Pro** | 41-65 | Multi-step analysis, code review | Capable (gpt-4o, sonnet) |
| **Frontier** | 66+ | Security audits, high-stakes | Best (gpt-4, opus) |

### TaskComplexity Enum

```go
type TaskComplexity int
const (
    TaskSimple   TaskComplexity = iota // Flash + Standard → cheap model
    TaskModerate                       // Pro → escalate if uncertain
    TaskComplex                        // Frontier → primary model
)
```

### 13-Dimension Scorer

Each dimension scores 0-100, weighted and combined:

| # | Dimension | Weight | What It Looks For |
|---|---|---|---|
| 1 | `reasoning_words` | 14% | "why", "explain", "analyze", "trade-offs", "compare" |
| 2 | `token_estimate` | 12% | Character count as proxy for response length |
| 3 | `code_indicators` | 10% | Functions, classes, imports, code blocks, `\`\`\`` |
| 4 | `multi_step` | 10% | "first", "then", "next", "steps", "pipeline" |
| 5 | `domain_specific` | 10% | "kubernetes", "docker", "react", "postgres", etc. |
| 6 | `ambiguity` | 5% | Vague pronouns ("it", "this", "that") |
| 7 | `creativity` | 7% | "write", "create", "generate", "compose", "story" |
| 8 | `precision` | 6% | Numbers, dates, "exactly", "calculate" |
| 9 | `context_dependency` | 5% | "previous", "mentioned", "recall" |
| 10 | `tool_likelihood` | 5% | "file", "read", "search", "execute", "deploy" |
| 11 | `safety_sensitivity` | 4% | "password", "secret", "auth", "token" |
| 12 | `question_complexity` | 7% | Multiple questions, open-ended |
| 13 | `sentence_complexity` | 5% | Commas, semicolons, conjunctions |

### Scoring Algorithm

```go
type ScoreBreakdown struct {
    Dimensions map[string]float64 // dimension → raw score (0-100)
    Total      float64            // weighted sum
    Tier       string             // "flash", "standard", "pro", "frontier"
}

func ScoreComplexity(prompt string) ScoreBreakdown {
    // Step 1: Pattern overrides (fast path)
    lower := strings.ToLower(prompt)

    // Greetings → Flash
    for _, g := range []string{"hello", "hi", "hey", "thanks", "thank you"} {
        if strings.TrimSpace(lower) == g { return ScoreBreakdown{Total: 5, Tier: "flash"} }
    }

    // Security audit → Frontier
    if strings.Contains(lower, "security audit") { return ScoreBreakdown{Total: 80, Tier: "frontier"} }

    // Deploy production → Pro
    if strings.Contains(lower, "deploy") && strings.Contains(lower, "production") {
        return ScoreBreakdown{Total: 55, Tier: "pro"}
    }

    // Explicit tier hints: [tier:pro]
    if match := regexp.MustCompile(`\[tier:(\w+)\]`).FindStringSubmatch(prompt); match != nil {
        return ScoreBreakdown{Total: tierToScore(match[1]), Tier: match[1]}
    }

    // Step 2: Score each dimension
    dims := map[string]float64{
        "reasoning_words":    scoreReasoningWords(prompt),
        "token_estimate":     scoreTokenEstimate(prompt),
        "code_indicators":    scoreCodeIndicators(prompt),
        "multi_step":         scoreMultiStep(prompt),
        "domain_specific":    scoreDomainSpecific(prompt),
        "ambiguity":          scoreAmbiguity(prompt),
        "creativity":         scoreCreativity(prompt),
        "precision":          scorePrecision(prompt),
        "context_dependency": scoreContextDependency(prompt),
        "tool_likelihood":    scoreToolLikelihood(prompt),
        "safety_sensitivity": scoreSafetySensitivity(prompt),
        "question_complexity":scoreQuestionComplexity(prompt),
        "sentence_complexity":scoreSentenceComplexity(prompt),
    }

    // Step 3: Weighted sum
    weights := map[string]float64{
        "reasoning_words": 0.14, "token_estimate": 0.12, "code_indicators": 0.10,
        "multi_step": 0.10, "domain_specific": 0.10, "ambiguity": 0.05,
        "creativity": 0.07, "precision": 0.06, "context_dependency": 0.05,
        "tool_likelihood": 0.05, "safety_sensitivity": 0.04,
        "question_complexity": 0.07, "sentence_complexity": 0.05,
    }

    total := 0.0
    for dim, score := range dims {
        total += score * weights[dim]
    }

    // Step 4: Multi-dimensional boost
    highDims := 0
    for _, score := range dims {
        if score > 20 { highDims++ }
    }
    if highDims >= 3 { total *= 1.30 } // +30%
    else if highDims >= 2 { total *= 1.15 } // +15%

    // Step 5: Classify
    tier := classifyScore(total)
    return ScoreBreakdown{Dimensions: dims, Total: total, Tier: tier}
}

func classifyScore(score float64) string {
    if score <= 15 { return "flash" }
    if score <= 40 { return "standard" }
    if score <= 65 { return "pro" }
    return "frontier"
}
```

### Uncertainty Detection (lines 520-551)

After a cheap model responds, check if it's uncertain:

```go
func ResponseIsUncertain(content string) bool {
    lower := strings.ToLower(content)
    uncertainPhrases := []string{
        "i don't know",
        "i'm not sure",
        "beyond my capabilities",
        "i cannot determine",
        "i'm unable to",
        "this is outside my",
        "i lack the ability",
    }
    for _, phrase := range uncertainPhrases {
        if strings.Contains(lower, phrase) { return true }
    }
    return false
}
```

If uncertain → escalate to next tier model.

## Cost Guard (cost_guard.rs)

### Structure

```go
type CostGuard struct {
    mu             sync.Mutex
    config         CostGuardConfig
    dailyCost      DailyCost
    actionWindow   []time.Time // sliding window for hourly rate
    budgetExceeded atomic.Bool // fast-path flag
    modelTokens    map[string]*ModelTokens
}

type CostGuardConfig struct {
    MaxCostPerDayCents *int  // e.g., 10000 = $100
    MaxActionsPerHour  *int  // e.g., 100
}

type DailyCost struct {
    TotalUSD  float64
    ResetDate time.Time // resets at midnight UTC
}

type ModelTokens struct {
    InputTokens  int
    OutputTokens int
    CostUSD      float64
}
```

### Check Before Request

```go
func (cg *CostGuard) CheckAllowed() error {
    // Fast path: atomic flag
    if cg.budgetExceeded.Load() { return ErrDailyBudgetExceeded }

    cg.mu.Lock()
    defer cg.mu.Unlock()

    // Reset daily if new day
    today := time.Now().UTC().Truncate(24 * time.Hour)
    if cg.dailyCost.ResetDate.Before(today) {
        cg.dailyCost = DailyCost{ResetDate: today}
        cg.budgetExceeded.Store(false)
    }

    // Daily budget check
    if cg.config.MaxCostPerDayCents != nil {
        limitUSD := float64(*cg.config.MaxCostPerDayCents) / 100.0
        if cg.dailyCost.TotalUSD >= limitUSD {
            return &CostLimitError{Type: "daily", Spent: cg.dailyCost.TotalUSD, Limit: limitUSD}
        }
    }

    // Hourly rate check (sliding window)
    if cg.config.MaxActionsPerHour != nil {
        oneHourAgo := time.Now().Add(-1 * time.Hour)
        // Prune old entries
        cg.actionWindow = filterAfter(cg.actionWindow, oneHourAgo)
        if len(cg.actionWindow) >= *cg.config.MaxActionsPerHour {
            return &CostLimitError{Type: "hourly", Actions: len(cg.actionWindow), Limit: *cg.config.MaxActionsPerHour}
        }
    }

    return nil
}
```

### Record After Request

```go
func (cg *CostGuard) RecordLLMCall(modelID string, inputTokens, outputTokens int) {
    cg.mu.Lock()
    defer cg.mu.Unlock()

    // Look up model cost
    inputRate, outputRate := ModelCost(modelID)

    // Calculate cost
    cost := (inputRate * float64(inputTokens)) + (outputRate * float64(outputTokens))

    // Update daily total
    cg.dailyCost.TotalUSD += cost

    // Check if exceeded (set fast-path flag)
    if cg.config.MaxCostPerDayCents != nil {
        limitUSD := float64(*cg.config.MaxCostPerDayCents) / 100.0
        if cg.dailyCost.TotalUSD >= limitUSD {
            cg.budgetExceeded.Store(true)
        }
        // Warn at 80% threshold
        if cg.dailyCost.TotalUSD >= limitUSD*0.8 {
            log.Warn("approaching daily budget", "spent", cg.dailyCost.TotalUSD, "limit", limitUSD)
        }
    }

    // Add to action window
    cg.actionWindow = append(cg.actionWindow, time.Now())

    // Track per-model usage
    mt := cg.modelTokens[modelID]
    if mt == nil { mt = &ModelTokens{}; cg.modelTokens[modelID] = mt }
    mt.InputTokens += inputTokens
    mt.OutputTokens += outputTokens
    mt.CostUSD += cost
}
```

## Cost Lookup Table (costs.rs)

```go
// Returns (input_cost_per_token, output_cost_per_token)
func ModelCost(modelID string) (float64, float64) {
    costs := map[string][2]float64{
        // OpenAI
        "gpt-4.1":       {0.000002, 0.000008},
        "gpt-4.1-mini":  {0.0000004, 0.0000016},
        "gpt-4o":        {0.0000025, 0.00001},
        "gpt-4o-mini":   {0.00000015, 0.0000006},
        "gpt-4-turbo":   {0.00001, 0.00003},
        "o3":            {0.00001, 0.00004},
        "o3-mini":       {0.0000011, 0.0000044},

        // Anthropic
        "claude-opus-4":    {0.000015, 0.000075},
        "claude-sonnet-4":  {0.000003, 0.000015},
        "claude-haiku-4":   {0.0000008, 0.000004},

        // Local models (free)
        // Detected by prefix: llama, mistral, phi, gemma, qwen, deepseek
        // Or suffix: :latest, :instruct
    }

    if c, ok := costs[modelID]; ok {
        return c[0], c[1]
    }

    // Check prefixes for local models
    localPrefixes := []string{"llama", "mistral", "mixtral", "phi", "gemma", "qwen", "deepseek"}
    lower := strings.ToLower(modelID)
    for _, prefix := range localPrefixes {
        if strings.HasPrefix(lower, prefix) { return 0, 0 }
    }

    // OpenRouter free models
    if strings.HasSuffix(lower, ":free") { return 0, 0 }

    return 0, 0 // unknown = free (conservative: won't block)
}
```

## Response Cache (cache.rs)

In-memory cache for deterministic requests:

```go
type ResponseCache struct {
    mu           sync.Mutex
    cache        map[string]*CacheEntry
    config       CacheConfig
    requestCount int64
    hitCount     int64
}

type CacheConfig struct {
    TTL        time.Duration // default 1 hour
    MaxEntries int           // default 1000
}

type CacheEntry struct {
    Response     CompletionResponse
    CreatedAt    time.Time
    LastAccessed time.Time
}

func (c *ResponseCache) Lookup(req CompletionRequest) (*CompletionResponse, bool) {
    // Skip if request has tools (side effects)
    if len(req.Tools) > 0 { return nil, false }

    key := c.cacheKey(req)

    c.mu.Lock()
    defer c.mu.Unlock()

    c.requestCount++
    entry, ok := c.cache[key]
    if !ok { return nil, false }

    // Check TTL
    if time.Since(entry.CreatedAt) > c.config.TTL {
        delete(c.cache, key)
        return nil, false
    }

    c.hitCount++
    entry.LastAccessed = time.Now()
    return &entry.Response, true
}

func (c *ResponseCache) cacheKey(req CompletionRequest) string {
    h := sha256.New()
    h.Write([]byte(req.ModelID))
    for _, msg := range req.Messages {
        h.Write([]byte(msg.Role))
        h.Write([]byte(msg.Content))
    }
    if req.MaxTokens != nil { binary.Write(h, binary.LittleEndian, *req.MaxTokens) }
    if req.Temperature != nil { binary.Write(h, binary.LittleEndian, *req.Temperature) }
    if req.System != nil { h.Write([]byte(*req.System)) }
    return hex.EncodeToString(h.Sum(nil))
}
```

## Integration Flow

```
User message: "hello"
    │
    ├── ScoreComplexity("hello")
    │   └── Pattern match: greeting → Flash (score=5)
    │
    ├── CostGuard.CheckAllowed()
    │   └── Budget OK ✓
    │
    ├── Select model: gpt-4o-mini (cheapest)
    ├── ResponseCache.Lookup(req) → miss
    ├── FailoverChain.Complete(req) → response
    │
    ├── CostGuard.RecordLLMCall("gpt-4o-mini", 50, 100)
    │   └── Cost: $0.000075 (negligible)
    │
    └── ResponseCache.Store(req, response)

---

User message: "Perform a comprehensive security audit of our Kubernetes deployment"
    │
    ├── ScoreComplexity(...)
    │   ├── Pattern: "security audit" → Frontier (score=80)
    │
    ├��─ CostGuard.CheckAllowed()
    │   └── Budget OK ✓
    │
    ├── Select model: claude-opus-4 (best available)
    ├── FailoverChain.Complete(req) → response
    │
    ├── CostGuard.RecordLLMCall("claude-opus-4", 2000, 5000)
    │   └── Cost: $0.405 (significant)
    │
    └── No cache (tools likely involved)
```

## Go Implementation Notes

1. **Regex patterns:** Compile once at startup with `regexp.MustCompile()` (equivalent to Rust's `LazyLock`)
2. **Atomic budget flag:** Use `atomic.Bool` for fast-path budget check (avoids mutex lock on every request)
3. **Sliding window:** `[]time.Time` with periodic cleanup is fine for hourly rate limiting
4. **Cache eviction:** Simple LRU via `LastAccessed` field + periodic cleanup goroutine
5. **Smart routing is optional** - can start with simple model selection and add complexity scorer later
