# 08 - Long-Term Memory (Vector + FTS5)

## Overview

Long-term memory provides **semantic search** over a knowledge base. Documents are chunked, embedded into vectors, and stored in SQLite with FTS5 for keyword search. Hybrid search combines BM25 (text) and cosine similarity (vector) for relevance.

This is how the agent "learns" from files and past knowledge - it can recall relevant information across sessions.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-memory/src/lib.rs` | 1-85 | `MemoryStore` trait, `ChunkEntry`, `SearchResult`, `SearchOptions` |
| `crates/frankclaw-memory/src/store.rs` | 1-516 | SQLite + FTS5 + vector store implementation |
| `crates/frankclaw-memory/src/embedding.rs` | 1-603 | 5 embedding providers + caching layer |
| `crates/frankclaw-memory/src/chunking.rs` | 1-119 | Text chunking by paragraphs |
| `crates/frankclaw-memory/src/sync.rs` | 1-225 | File system → memory store synchronization |
| `crates/frankclaw-core/src/tool_services.rs` | 51-62 | `MemorySearch` trait (used by tools) |
| `crates/frankclaw-tools/src/memory.rs` | 1-266 | `memory_get` and `memory_search` tools |

## Database Schema (store.rs:37-103)

### GORM Models

```go
type MemoryChunk struct {
    ID          string    `gorm:"primaryKey;size:255"`
    Source      string    `gorm:"index;size:500;not null"`
    Text        string    `gorm:"type:text;not null"`
    LineStart   int       `gorm:"not null"`
    LineEnd     int       `gorm:"not null"`
    ChunkIndex  int       `gorm:"not null"`
    ContentHash string    `gorm:"size:64;default:''"`
    CreatedAt   time.Time `gorm:"default:CURRENT_TIMESTAMP"`
}

type MemoryEmbedding struct {
    ChunkID   string `gorm:"primaryKey;size:255"` // FK → memory_chunks.id
    Embedding []byte `gorm:"type:blob;not null"`   // f32 array as little-endian bytes
}

type MemorySourceHash struct {
    Source      string `gorm:"primaryKey;size:500"`
    ContentHash string `gorm:"size:64;not null"`
}
```

### FTS5 Virtual Table (must create manually, not via GORM)

```sql
CREATE VIRTUAL TABLE memory_fts USING fts5(
    text,
    content='memory_chunks',
    content_rowid='rowid'
);

-- Triggers to keep FTS in sync with chunks table
CREATE TRIGGER memory_chunks_ai AFTER INSERT ON memory_chunks
BEGIN
    INSERT INTO memory_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER memory_chunks_ad AFTER DELETE ON memory_chunks
BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, text) VALUES('delete', old.rowid, old.text);
END;

CREATE TRIGGER memory_chunks_au AFTER UPDATE ON memory_chunks
BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, text) VALUES('delete', old.rowid, old.text);
    INSERT INTO memory_fts(rowid, text) VALUES (new.rowid, new.text);
END;
```

## MemoryStore Trait (lib.rs:58-77)

```go
type MemoryStore interface {
    StoreChunk(ctx context.Context, chunk ChunkEntry, embedding []float32) error
    SearchHybrid(ctx context.Context, query string, queryEmbedding []float32, options SearchOptions) ([]SearchResult, error)
    DeleteBySource(ctx context.Context, source string) (int, error)
    ListSources(ctx context.Context) ([]SourceInfo, error)
    HasSource(ctx context.Context, source string) (bool, error)
}
```

### Types

```go
type ChunkEntry struct {
    ID         string    `json:"id"`
    Source     string    `json:"source"`      // file path or origin
    Text       string    `json:"text"`
    LineStart  int       `json:"line_start"`
    LineEnd    int       `json:"line_end"`
    ChunkIndex int       `json:"chunk_index"` // order within source
    CreatedAt  time.Time `json:"created_at"`
}

type SearchResult struct {
    Chunk ChunkEntry `json:"chunk"`
    Score float32    `json:"score"` // combined relevance
}

type SearchOptions struct {
    Limit        int     `json:"limit"`         // default 10
    MinScore     float32 `json:"min_score"`     // default 0.0
    VectorWeight float32 `json:"vector_weight"` // 0.0-1.0, default 0.6
}

type SourceInfo struct {
    Source      string `json:"source"`
    ChunkCount int    `json:"chunk_count"`
    ContentHash string `json:"content_hash"` // SHA-256 for change detection
}
```

## Hybrid Search Algorithm (store.rs:143-288)

This is the core of long-term memory retrieval:

```go
func (s *SqliteMemoryStore) SearchHybrid(ctx context.Context, query string, queryEmbedding []float32, opts SearchOptions) ([]SearchResult, error) {
    // ═══════════════════════════════════════════
    // STEP 1: BM25 Text Search (40% weight default)
    // ═══════════════════════════════════════════
    escapedQuery := EscapeFts5(query) // escape special chars
    quotedQuery := `"` + escapedQuery + `"`

    // SELECT mc.id, bm25(memory_fts) as score
    // FROM memory_fts
    // JOIN memory_chunks mc ON mc.rowid = memory_fts.rowid
    // WHERE memory_fts MATCH ?
    // ORDER BY score  -- bm25 returns negative (lower=better)
    // LIMIT opts.Limit * 3  -- oversample for re-ranking

    // Normalize BM25 scores to positive (negate + normalize to 0-1)
    bm25Scores := map[string]float32{}  // chunk_id → normalized score

    // ═══════════════════════════════════════════
    // STEP 2: Vector Similarity (60% weight default)
    // ═══════════════════════════════════════════
    // Load ALL embeddings (brute-force cosine similarity)
    // SELECT chunk_id, embedding FROM memory_embeddings

    vectorScores := map[string]float32{}
    for _, row := range embeddings {
        vec := BytesToF32(row.Embedding)
        sim := CosineSimilarity(queryEmbedding, vec)
        if sim > 0.0 {
            vectorScores[row.ChunkID] = sim
        }
    }

    // ═══════════════════════════════════════════
    // STEP 3: Score Merging
    // ═══════════════════════════════════════════
    allChunkIDs := union(keys(bm25Scores), keys(vectorScores))
    merged := []scoredChunk{}
    for _, id := range allChunkIDs {
        combined := vectorScores[id]*opts.VectorWeight + bm25Scores[id]*(1.0-opts.VectorWeight)
        if combined >= opts.MinScore {
            merged = append(merged, scoredChunk{id, combined})
        }
    }

    // Sort descending by score, truncate to limit
    sort.Slice(merged, func(i, j int) bool { return merged[i].score > merged[j].score })
    if len(merged) > opts.Limit { merged = merged[:opts.Limit] }

    // ═══════════════════════════════════════════
    // STEP 4: Load Full Chunk Data
    // ═══════════════════════════════════════════
    // SELECT * FROM memory_chunks WHERE id IN (?)
    // Return SearchResult with chunk + score

    return results, nil
}
```

### Cosine Similarity (store.rs:403-421)

```go
func CosineSimilarity(a, b []float32) float32 {
    if len(a) != len(b) { return 0 }
    var dot, normA, normB float32
    for i := range a {
        dot += a[i] * b[i]
        normA += a[i] * a[i]
        normB += b[i] * b[i]
    }
    denom := float32(math.Sqrt(float64(normA))) * float32(math.Sqrt(float64(normB)))
    if denom == 0 { return 0 }
    return dot / denom
}
```

### Vector Serialization (store.rs:423-431)

```go
func F32ToBytes(vec []float32) []byte {
    buf := make([]byte, len(vec)*4)
    for i, v := range vec {
        binary.LittleEndian.PutUint32(buf[i*4:], math.Float32bits(v))
    }
    return buf
}

func BytesToF32(data []byte) []float32 {
    vec := make([]float32, len(data)/4)
    for i := range vec {
        vec[i] = math.Float32frombits(binary.LittleEndian.Uint32(data[i*4:]))
    }
    return vec
}
```

## Text Chunking (chunking.rs:3-54)

Splits documents into chunks by paragraph boundaries:

```go
func ChunkText(text string, targetSize int) []Chunk {
    if targetSize == 0 { targetSize = 1500 } // default

    lines := strings.Split(text, "\n")
    var chunks []Chunk
    var current strings.Builder
    lineStart := 1 // 1-indexed
    currentStart := 1
    chunkIndex := 0

    for i, line := range lines {
        lineNum := i + 1

        // Blank line = paragraph boundary
        if strings.TrimSpace(line) == "" && current.Len() >= targetSize {
            chunks = append(chunks, Chunk{
                Text:      current.String(),
                LineStart: currentStart,
                LineEnd:   lineNum - 1,
                Index:     chunkIndex,
            })
            current.Reset()
            currentStart = lineNum + 1
            chunkIndex++
            continue
        }

        current.WriteString(line + "\n")
    }

    // Flush remaining
    if current.Len() > 0 {
        chunks = append(chunks, Chunk{
            Text:      current.String(),
            LineStart: currentStart,
            LineEnd:   len(lines),
            Index:     chunkIndex,
        })
    }

    return chunks
}
```

## Embedding Providers (embedding.rs:1-603)

### Interface

```go
type EmbeddingProvider interface {
    Embed(ctx context.Context, text string) ([]float32, error)
    EmbedBatch(ctx context.Context, texts []string) ([][]float32, error)
    Dimension() int
    ModelName() string
}
```

### Implementations

| Provider | Model | Dimension | Batch Size | File Lines |
|---|---|---|---|---|
| **OpenAI** | text-embedding-3-small | 1536 | 100 | 18-121 |
| **OpenAI** | text-embedding-3-large | 3072 | 100 | 18-121 |
| **Ollama** | configurable | configurable | 1 (sequential) | 124-198 |
| **Gemini** | text-embedding-004 | 768 | batch API | 318-426 |
| **Voyage AI** | voyage-3 | 1024 | batch API | 429-528 |

### Cached Embedding Provider (embedding.rs:208-315)

Wraps any provider with an SQLite cache:

```go
type CachedEmbeddingProvider struct {
    Inner EmbeddingProvider
    Cache *sql.DB // SQLite
}

// Cache table:
// CREATE TABLE embedding_cache (
//     text_hash TEXT NOT NULL,
//     model TEXT NOT NULL,
//     embedding BLOB NOT NULL,
//     PRIMARY KEY (text_hash, model)
// )

func (c *CachedEmbeddingProvider) Embed(ctx context.Context, text string) ([]float32, error) {
    hash := sha256Hex(text)
    // Try cache first
    cached, err := c.lookup(hash)
    if err == nil { return cached, nil }
    // Miss: call inner provider
    vec, err := c.Inner.Embed(ctx, text)
    if err != nil { return nil, err }
    // Store in cache
    c.store(hash, vec)
    return vec, nil
}

func (c *CachedEmbeddingProvider) EmbedBatch(ctx context.Context, texts []string) ([][]float32, error) {
    // Batch-aware: only fetch uncached texts from inner provider
    results := make([][]float32, len(texts))
    var uncachedIdx []int
    var uncachedTexts []string

    for i, text := range texts {
        hash := sha256Hex(text)
        cached, err := c.lookup(hash)
        if err == nil {
            results[i] = cached
        } else {
            uncachedIdx = append(uncachedIdx, i)
            uncachedTexts = append(uncachedTexts, text)
        }
    }

    if len(uncachedTexts) > 0 {
        vecs, err := c.Inner.EmbedBatch(ctx, uncachedTexts)
        if err != nil { return nil, err }
        for j, idx := range uncachedIdx {
            results[idx] = vecs[j]
            c.store(sha256Hex(texts[idx]), vecs[j])
        }
    }

    return results, nil
}
```

## Memory Syncer (sync.rs:1-225)

Synchronizes a directory of files into the memory store:

```go
type MemorySyncer struct {
    Store     MemoryStore
    Embedder  EmbeddingProvider
    MemoryDir string
    ChunkSize int
}

type SyncReport struct {
    Indexed int
    Skipped int
    Removed int
    Errors  int
}

func (s *MemorySyncer) SyncOnce(ctx context.Context) (*SyncReport, error) {
    report := &SyncReport{}

    // 1. Scan directory recursively for supported files
    files := scanDirectory(s.MemoryDir)

    // 2. Get existing sources from store
    existing, _ := s.Store.ListSources(ctx)
    existingMap := toMap(existing) // source → content_hash

    // 3. Process each file
    for _, file := range files {
        hash := sha256File(file)

        // Skip if hash matches existing
        if existingMap[file] == hash {
            report.Skipped++
            continue
        }

        // Delete old chunks
        s.Store.DeleteBySource(ctx, file)

        // Read + chunk
        content := readFile(file)
        chunks := ChunkText(content, s.ChunkSize)

        // Batch embed
        texts := extractTexts(chunks)
        embeddings, err := s.Embedder.EmbedBatch(ctx, texts)
        if err != nil {
            report.Errors++
            continue
        }

        // Store new chunks
        for i, chunk := range chunks {
            entry := ChunkEntry{
                ID:         uuid.New().String(),
                Source:     file,
                Text:       chunk.Text,
                LineStart:  chunk.LineStart,
                LineEnd:    chunk.LineEnd,
                ChunkIndex: chunk.Index,
            }
            s.Store.StoreChunk(ctx, entry, embeddings[i])
        }
        report.Indexed++
    }

    // 4. Detect deleted files (in store but not on disk)
    for source := range existingMap {
        if !fileExists(source) {
            s.Store.DeleteBySource(ctx, source)
            report.Removed++
        }
    }

    return report, nil
}
```

**Supported file types:** `.md`, `.txt`, `.rs`, `.py`, `.js`, `.ts`, `.json`, `.yaml`, `.toml`, `.cfg`, `.ini`, `.csv`, `.html`, `.xml`, `.sh`, `.bash`, `.zsh`

## How Long-Term Memory Connects to the Agentic Loop

```
Agent has "memory_search" in allowed_tools
    │
    v
Model calls memory_search(query="how to deploy")
    │
    v
MemorySearchTool.Invoke()
    │
    ├── ctx.MemorySearch.Search(query, limit=10)
    │       │
    │       └── MemoryStore.SearchHybrid(query, embedding, options)
    │               │
    │               ├── FTS5 BM25 search (keyword matching)
    │               ├── Vector cosine similarity (semantic matching)
    │               └── Merged + ranked results
    │
    └── Returns ranked results to model as tool output:
        [
          {"source": "docs/deploy.md", "text": "...", "score": 0.87, "line_start": 15},
          {"source": "notes/infra.md", "text": "...", "score": 0.72, "line_start": 3}
        ]
```

The model then uses these results to inform its response, effectively "remembering" knowledge from files it was previously trained on.

## Memory Layers Summary

| Layer | Storage | Scope | Duration | Mechanism |
|---|---|---|---|---|
| **Short-term** | RAM ([]CompletionMessage) | Single turn | Minutes | Token-limited context window |
| **Medium-term** | SQLite transcript | Session | Days-weeks | Encrypted, paginated, prunable |
| **Long-term** | SQLite FTS5 + vectors | Agent | Permanent | Hybrid search, file sync |

## Go Implementation Notes

1. **FTS5** is a SQLite extension - use `github.com/mattn/go-sqlite3` with CGO and `SQLITE_ENABLE_FTS5`
2. **GORM + raw SQL:** Use GORM for CRUD but raw SQL for FTS5 queries and virtual tables
3. **Embedding cache:** SQLite is fine for this; separate DB from main store
4. **Brute-force cosine:** Fine for <100K chunks. For larger: consider pgvector or faiss-go
5. **Chunk size 1500 chars** is a good default - roughly 400 tokens
6. **SHA-256 content hashing** enables efficient incremental sync (only re-index changed files)
7. **f32 as bytes:** Little-endian binary is compact and fast to serialize
