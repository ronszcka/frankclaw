# 07 - Medium-Term Memory (Session Persistence)

## Overview

Medium-term memory persists conversation history across multiple turns within a session. Sessions last days to weeks, stored in SQLite with optional encryption at rest. This is what makes the agent "remember" previous messages in the same conversation.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-core/src/session.rs` | 1-200 | `SessionStore` trait, `SessionEntry`, `TranscriptEntry`, scoping |
| `crates/frankclaw-sessions/src/store.rs` | 1-900+ | SQLite implementation with encryption |
| `crates/frankclaw-sessions/src/migrations.rs` | 1-53 | Schema DDL |
| `crates/frankclaw-crypto/src/encryption.rs` | 1-100 | ChaCha20-Poly1305 for transcript encryption |
| `crates/frankclaw-crypto/src/keys.rs` | 1-100 | Key derivation for session subkey |
| `crates/frankclaw-runtime/src/lib.rs` | 313-331 | Session loading in chat() |
| `crates/frankclaw-runtime/src/lib.rs` | 430-481 | Transcript→CompletionMessage conversion |
| `crates/frankclaw-runtime/src/lib.rs` | 541-605 | Persisting assistant messages + tool results |

## Database Schema (migrations.rs)

### Sessions Table

```sql
CREATE TABLE sessions (
    key TEXT PRIMARY KEY,            -- "agent_id:channel:account_id"
    agent_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    account_id TEXT NOT NULL,
    scoping TEXT DEFAULT 'main',     -- main, per_peer, per_channel_peer, global
    thread_id TEXT,
    metadata TEXT DEFAULT '{}',      -- JSON extensible metadata
    created_at TEXT NOT NULL,        -- ISO 8601
    last_message_at TEXT             -- updated on every message
);

CREATE INDEX idx_sessions_agent ON sessions(agent_id);
CREATE INDEX idx_sessions_channel_account ON sessions(channel, account_id);
CREATE INDEX idx_sessions_last_message ON sessions(last_message_at);
```

### Transcript Table

```sql
CREATE TABLE transcript (
    session_key TEXT NOT NULL,
    seq INTEGER NOT NULL,             -- monotonically increasing per session
    role TEXT NOT NULL,               -- "user", "assistant", "tool"
    content BLOB NOT NULL,            -- encrypted if master key provided
    metadata TEXT,                    -- JSON: tool_name, tool_call_id, attachments, etc.
    timestamp TEXT NOT NULL,          -- ISO 8601
    PRIMARY KEY (session_key, seq),
    FOREIGN KEY (session_key) REFERENCES sessions(key) ON DELETE CASCADE
);

CREATE INDEX idx_transcript_session_seq ON transcript(session_key, seq);
```

### GORM Models

```go
type Session struct {
    Key           string         `gorm:"primaryKey;size:800"`
    AgentID       string         `gorm:"index;size:255;not null"`
    Channel       string         `gorm:"size:255;not null"`
    AccountID     string         `gorm:"size:255;not null"`
    Scoping       string         `gorm:"size:50;default:'main'"`
    ThreadID      *string        `gorm:"size:255"`
    Metadata      datatypes.JSON `gorm:"type:text;default:'{}'"`
    CreatedAt     time.Time      `gorm:"not null"`
    LastMessageAt *time.Time
}

type Transcript struct {
    SessionKey string         `gorm:"primaryKey;size:800"`
    Seq        uint64         `gorm:"primaryKey"`
    Role       string         `gorm:"size:20;not null"`
    Content    []byte         `gorm:"type:blob;not null"` // encrypted if key set
    Metadata   datatypes.JSON `gorm:"type:text"`
    Timestamp  time.Time      `gorm:"not null"`
}
```

## SQLite Configuration (store.rs:25-89)

| Setting | Value | Purpose |
|---|---|---|
| WAL mode | ON | Concurrent reads during writes |
| Foreign keys | ON | CASCADE on session delete |
| Secure delete | ON | Overwrite deleted data |
| File permissions | 0o600 | Owner-only read/write |
| Directory permissions | 0o700 | Owner-only access |
| Connection pool | 8 max, 5s timeout | r2d2 pool |

## Session Scoping (session.rs:10-26)

Determines how sessions are isolated per user:

| Mode | Key Format | Use Case |
|---|---|---|
| `Main` | `agent:channel:account` | One session per sender (default) |
| `PerPeer` | `agent:channel:peer` | Separate per DM peer |
| `PerChannelPeer` | `agent:channel:account:peer` | Per channel + peer (groups) |
| `Global` | `agent:channel:__global__` | Single shared session |

## Encryption at Rest (store.rs:92-135)

When a `MasterKey` is configured, transcript content is encrypted:

```go
// Encryption
func EncryptContent(key []byte, plaintext string) ([]byte, error) {
    blob, err := crypto.Encrypt(key, []byte(plaintext))
    if err != nil { return nil, err }
    return json.Marshal(blob) // Store as JSON: {"nonce": "...", "ciphertext": "..."}
}

// Decryption
func DecryptContent(key []byte, data []byte) (string, error) {
    var blob crypto.EncryptedBlob
    if err := json.Unmarshal(data, &blob); err != nil {
        return string(data), nil // Not encrypted, return as-is
    }
    plaintext, err := crypto.Decrypt(key, &blob)
    if err != nil { return "", fmt.Errorf("decryption failed (key rotation?)") }
    return string(plaintext), nil
}
```

**Key derivation:** `subkey = HMAC-SHA256(HMAC-SHA256(master, "frankclaw-kdf"), "session" || 0x01)`

## SessionStore Implementation

### AppendTranscript (store.rs:347-386)

```go
func (s *SqliteSessionStore) AppendTranscript(ctx context.Context, key SessionKey, entry *TranscriptEntry) error {
    // 1. Validate entry size (max 1 MB)
    if len(entry.Content) > 1_000_000 {
        return ErrRequestTooLarge
    }

    // 2. Encrypt content if key available
    content := entry.Content
    if s.encryptionKey != nil {
        content = EncryptContent(s.encryptionKey, entry.Content)
    }

    // 3. Insert transcript entry
    // INSERT INTO transcript (session_key, seq, role, content, metadata, timestamp) VALUES (?, ?, ?, ?, ?, ?)

    // 4. Update session's last_message_at
    // UPDATE sessions SET last_message_at = ? WHERE key = ?

    return nil
}
```

### GetTranscript (store.rs:388-467)

```go
func (s *SqliteSessionStore) GetTranscript(ctx context.Context, key SessionKey, limit int, beforeSeq *uint64) ([]TranscriptEntry, error) {
    // 1. Query in descending order (newest first) with cursor-based pagination
    // SELECT seq, role, content, metadata, timestamp
    // FROM transcript WHERE session_key = ?
    // AND (seq < ? if beforeSeq provided)
    // ORDER BY seq DESC LIMIT ?

    // 2. Decrypt all entries
    for i := range entries {
        entries[i].Content = DecryptContent(s.encryptionKey, entries[i].Content)
    }

    // 3. Reverse for ascending order
    slices.Reverse(entries)

    return entries, nil
}
```

### Maintenance (store.rs:487-535)

Automatic session cleanup:

```go
func (s *SqliteSessionStore) Maintenance(ctx context.Context, config PruningConfig) (uint64, error) {
    deleted := 0

    // 1. Delete sessions older than max_age_days
    if config.MaxAgeDays > 0 {
        cutoff := time.Now().AddDate(0, 0, -config.MaxAgeDays)
        // DELETE FROM sessions WHERE last_message_at < ? OR (last_message_at IS NULL AND created_at < ?)
        deleted += count
    }

    // 2. Enforce per-agent session limit (keep most recent N)
    if config.MaxSessionsPerAgent > 0 {
        // SELECT key FROM sessions WHERE agent_id = ?
        // ORDER BY last_message_at DESC
        // OFFSET max_sessions_per_agent
        // → DELETE those excess sessions
        deleted += count
    }

    return deleted, nil
}
```

### Session Reset Policy (session.rs:65-83)

Sessions can be automatically reset based on:

```go
type SessionResetPolicy struct {
    DailyAtHour    *int  // Reset at this UTC hour (0-23)
    IdleTimeoutSec *int  // Reset after N seconds of inactivity
    MaxEntries     *int  // Reset when transcript exceeds N entries
}
```

Checked in the runtime before loading transcript:
- If `DailyAtHour` set and session was last active on a different day → clear transcript
- If `IdleTimeoutSec` set and `now - last_message_at > timeout` → clear transcript
- If `MaxEntries` set and `transcript.len() > max` → clear transcript

## How Medium-Term Memory Feeds Short-Term

In `Runtime::chat()`:

```
1. sessions.GetTranscript(key, limit=200)    // Load last 200 messages
     │
     v
2. Convert TranscriptEntry → CompletionMessage
   for each entry:
     - role: entry.Role
     - content: entry.Content (decrypted)
     - tool_calls: from entry.Metadata["tool_calls"] if present
     - tool_call_id: from entry.Metadata["tool_call_id"] if present
     │
     v
3. Append current user message
     │
     v
4. OptimizeContext(messages, model, systemPrompt)
   → Prune if over token budget
     │
     v
5. Send to LLM as CompletionRequest.Messages
```

## Rewrite Last Assistant Message (store.rs:143-189)

For streaming corrections (when the assistant's response is updated):

```go
func (s *SqliteSessionStore) RewriteLastAssistantMessage(ctx context.Context, key SessionKey, newContent string) error {
    // Find latest Assistant message by role
    // UPDATE transcript SET content = ?, timestamp = ? WHERE session_key = ? AND seq = ?
}
```

## Go Implementation Notes

1. **Use GORM** for sessions and transcript tables - straightforward CRUD
2. **Encryption:** Use Go's `golang.org/x/crypto/chacha20poly1305` for encryption at rest
3. **Connection pool:** GORM handles this natively with `gorm.Open()` + `sql.DB.SetMaxOpenConns(8)`
4. **WAL mode:** Set via `db.Exec("PRAGMA journal_mode=WAL")`
5. **Cursor pagination:** Use `beforeSeq` for efficient pagination instead of OFFSET
6. **Transcript metadata:** Store as JSON in a TEXT column (tool_call info, attachments, etc.)
7. **The 200 message limit** is a good default - prevents loading too much history while providing context
