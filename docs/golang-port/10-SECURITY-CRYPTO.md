# 10 - Security & Cryptography

## Overview

FrankClaw has defense-in-depth security: input sanitization (prompt injection), encryption at rest (ChaCha20-Poly1305), password hashing (Argon2id), SSRF protection, credential leak detection, and constant-time token comparison.

## Files

| File | Lines | Role |
|---|---|---|
| `crates/frankclaw-crypto/src/lib.rs` | 1-38 | Public API + `CryptoError` enum |
| `crates/frankclaw-crypto/src/encryption.rs` | 1-100 | ChaCha20-Poly1305 encrypt/decrypt |
| `crates/frankclaw-crypto/src/keys.rs` | 1-100 | MasterKey (Argon2id) + HMAC-SHA256 subkeys |
| `crates/frankclaw-crypto/src/hashing.rs` | 1-88 | Password hashing (Argon2id) |
| `crates/frankclaw-crypto/src/token.rs` | 1-100 | Random token generation + constant-time compare |
| `crates/frankclaw-core/src/sanitize.rs` | 1-226 | Prompt injection prevention |
| `crates/frankclaw-core/src/links.rs` | 1-260 | URL extraction + SSRF protection |
| `crates/frankclaw-core/src/media.rs` | ~140-160 | `is_safe_ip()` SSRF IP blocklist |
| `crates/frankclaw-runtime/src/leak_detector.rs` | 1-150+ | Credential leak scanning in tool outputs |

## Encryption (encryption.rs)

### ChaCha20-Poly1305 (AEAD)

Used for: transcript encryption at rest, config secret storage.

```go
import "golang.org/x/crypto/chacha20poly1305"

type EncryptedBlob struct {
    Nonce      [12]byte `json:"nonce"`      // 96-bit random nonce
    Ciphertext []byte   `json:"ciphertext"` // encrypted + 16-byte auth tag
}

func Encrypt(key [32]byte, plaintext []byte) (*EncryptedBlob, error) {
    aead, err := chacha20poly1305.New(key[:])
    if err != nil { return nil, err }

    nonce := make([]byte, 12)
    if _, err := rand.Read(nonce); err != nil { return nil, err }

    ciphertext := aead.Seal(nil, nonce, plaintext, nil)

    blob := &EncryptedBlob{}
    copy(blob.Nonce[:], nonce)
    blob.Ciphertext = ciphertext
    return blob, nil
}

func Decrypt(key [32]byte, blob *EncryptedBlob) ([]byte, error) {
    aead, err := chacha20poly1305.New(key[:])
    if err != nil { return nil, err }

    plaintext, err := aead.Open(nil, blob.Nonce[:], blob.Ciphertext, nil)
    if err != nil { return nil, ErrDecryptionFailed }
    return plaintext, nil
}
```

## Key Derivation (keys.rs)

### MasterKey from Passphrase

```go
import "golang.org/x/crypto/argon2"

type MasterKey struct {
    bytes [32]byte
}

// Argon2id parameters: t=3 iterations, m=64MB, p=4 threads
func MasterKeyFromPassphrase(passphrase, salt string) (*MasterKey, error) {
    key := argon2.IDKey(
        []byte(passphrase),
        []byte(salt),
        3,          // time
        64*1024,    // memory (64MB)
        4,          // threads
        32,         // key length
    )
    mk := &MasterKey{}
    copy(mk.bytes[:], key)
    return mk, nil
}

func MasterKeyFromBytes(b [32]byte) *MasterKey {
    return &MasterKey{bytes: b}
}
```

### Subkey Derivation (HMAC-SHA256 KDF)

```go
import "crypto/hmac"
import "crypto/sha256"

func DeriveSubkey(master *MasterKey, context string) ([32]byte, error) {
    // Step 1: PRK = HMAC-SHA256(master, "frankclaw-kdf")
    mac := hmac.New(sha256.New, master.bytes[:])
    mac.Write([]byte("frankclaw-kdf"))
    prk := mac.Sum(nil)

    // Step 2: OKM = HMAC-SHA256(PRK, context || 0x01)
    mac2 := hmac.New(sha256.New, prk)
    mac2.Write([]byte(context))
    mac2.Write([]byte{0x01})
    okm := mac2.Sum(nil)

    var result [32]byte
    copy(result[:], okm)
    return result, nil
}
```

**Context strings used:** `"session"` (transcript encryption), `"config"` (config secrets), `"media"` (media encryption)

## Password Hashing (hashing.rs)

### Argon2id for Login Passwords

```go
import "golang.org/x/crypto/argon2"

type PasswordHash struct {
    Hash string // PHC format: $argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>
}

func HashPassword(password string) (*PasswordHash, error) {
    salt := make([]byte, 16)
    rand.Read(salt)

    hash := argon2.IDKey([]byte(password), salt, 3, 64*1024, 4, 32)

    // Encode as PHC format string
    phc := fmt.Sprintf("$argon2id$v=19$m=65536,t=3,p=4$%s$%s",
        base64.RawStdEncoding.EncodeToString(salt),
        base64.RawStdEncoding.EncodeToString(hash))

    return &PasswordHash{Hash: phc}, nil
}

func VerifyPassword(password string, stored *PasswordHash) (bool, error) {
    // Parse PHC string, extract params, salt, expected hash
    // Recompute with same params
    // Constant-time compare
}
```

## Token Generation & Comparison (token.rs)

```go
import "crypto/rand"
import "crypto/subtle"
import "encoding/base64"

// 32 random bytes → base64url (43 chars, 256-bit entropy)
func GenerateToken() string {
    b := make([]byte, 32)
    rand.Read(b)
    return base64.RawURLEncoding.EncodeToString(b)
}

// Constant-time comparison (prevents timing attacks)
func VerifyTokenEqual(provided, expected string) bool {
    return subtle.ConstantTimeCompare([]byte(provided), []byte(expected)) == 1
}
```

## Input Sanitization (sanitize.rs)

### sanitize_for_prompt() - Prompt Injection Prevention

Strips Unicode control characters (Cc category) and format characters (Cf category) that can be used for prompt injection:

```go
func SanitizeForPrompt(input string) string {
    var result strings.Builder
    for _, r := range input {
        // Preserve whitespace
        if r == '\t' || r == '\n' || r == '\r' {
            result.WriteRune(r)
            continue
        }
        // Strip control characters (Cc category)
        if unicode.IsControl(r) {
            continue
        }
        // Strip format characters (Cf category): zero-width chars, bidi overrides, soft hyphens
        if IsFormatChar(r) {
            continue
        }
        result.WriteRune(r)
    }
    return result.String()
}

func IsFormatChar(r rune) bool {
    // Zero-width space, zero-width non-joiner, zero-width joiner
    // Bidi overrides (LRO, RLO, LRE, RLE, PDF, LRI, RLI, FSI, PDI)
    // Soft hyphen, word joiner, byte order marks
    return unicode.Is(unicode.Cf, r)
}
```

### Boundary Wrapping

```go
// For user-provided text (not direct chat)
func WrapUntrustedText(text string) string {
    sanitized := SanitizeForPrompt(text)
    return "<untrusted-text>\n" + sanitized + "\n</untrusted-text>"
}

// For external/fetched content (URLs, APIs)
func WrapExternalContent(source, content string) string {
    sanitized := SanitizeForPrompt(content)
    return fmt.Sprintf("<external-content source=\"%s\">\n%s\n</external-content>",
        html.EscapeString(source), sanitized)
}
```

### Prompt Size Check

```go
const MaxPromptBytes = 2 * 1024 * 1024 // 2 MB hard cap

func CheckPromptSize(messages []CompletionMessage, system string) bool {
    total := len(system)
    for _, msg := range messages {
        total += len(msg.Content)
    }
    return total <= MaxPromptBytes
}
```

## SSRF Protection (links.rs, media.rs)

### IP Blocklist

```go
func IsSafeIP(ip net.IP) bool {
    // Block ALL of these:
    if ip.IsLoopback() { return false }          // 127.0.0.0/8
    if ip.IsPrivate() { return false }            // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
    if ip.IsLinkLocalUnicast() { return false }   // 169.254.0.0/16
    if ip.IsLinkLocalMulticast() { return false }

    // CGNAT range: 100.64.0.0/10
    if ip4 := ip.To4(); ip4 != nil {
        if ip4[0] == 100 && ip4[1] >= 64 && ip4[1] <= 127 { return false }
    }

    // Documentation ranges: 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24
    // Benchmarking: 198.18.0.0/15
    // 0.0.0.0/8
    // IPv6 mapped private IPv4

    return true
}
```

### URL Validation

```go
func IsAllowedURL(rawURL string) bool {
    u, err := url.Parse(rawURL)
    if err != nil { return false }

    // Only HTTP(S)
    if u.Scheme != "http" && u.Scheme != "https" { return false }

    // Resolve hostname to IP
    ips, err := net.LookupIP(u.Hostname())
    if err != nil { return false }

    // All resolved IPs must be safe
    for _, ip := range ips {
        if !IsSafeIP(ip) { return false }
    }

    // Block .local, .internal TLDs
    host := strings.ToLower(u.Hostname())
    if strings.HasSuffix(host, ".local") || strings.HasSuffix(host, ".internal") {
        return false
    }

    return true
}
```

## Credential Leak Detection (leak_detector.rs)

Scans tool outputs for accidentally exposed secrets:

```go
type LeakSeverity int
const (
    LeakMedium LeakSeverity = iota
    LeakHigh
    LeakCritical
)

type LeakAction int
const (
    LeakWarn LeakAction = iota
    LeakRedact
    LeakBlock
)

type LeakMatch struct {
    PatternName   string
    Severity      LeakSeverity
    Action        LeakAction
    MaskedPreview string // "sk-pr...xyz9"
}

type LeakScanResult struct {
    Matches        []LeakMatch
    ShouldBlock    bool
    RedactedContent *string
}
```

### Patterns Detected

| Pattern | Severity | Action | Example |
|---|---|---|---|
| OpenAI API key (`sk-`) | Critical | Block | `sk-proj-abc...xyz` |
| GitHub token (`ghp_`, `ghs_`, `ghu_`) | High | Redact | `ghp_aBcDeFgHiJ...` |
| PEM private key | Critical | Block | `-----BEGIN PRIVATE KEY-----` |
| AWS access key (`AKIA`) | Critical | Block | `AKIAIOSFODNN7EXAMPLE` |
| Generic API key pattern | Medium | Warn | `api_key=abc123...` |

### Masking

```go
func MaskSecret(secret string) string {
    if len(secret) <= 8 {
        return "****"
    }
    return secret[:4] + "..." + secret[len(secret)-4:]
}
```

## Security Interactions

```
User message
    │
    ├── SanitizeForPrompt() ← Strip control chars
    ├── CheckPromptSize() ← 2MB hard cap
    │
    v
[Agentic Loop]
    │
    ├── Tool: web_fetch(url)
    │   └── IsAllowedURL(url) + IsSafeIP() ← SSRF protection
    │
    ├── Tool: bash(command)
    │   └── BashPolicy check + metacharacter filter ← Command injection prevention
    │
    ├── Tool: file_read(path)
    │   └── validate_workspace_path() ← Path traversal prevention
    │
    ├── Tool output
    │   └── ScanForLeaks() ← Credential leak detection
    │       ├── Block → abort turn
    │       ├── Redact → mask secrets
    │       └── Warn → log only
    │
    ├── Session storage
    │   └── Encrypt(subkey, content) ← ChaCha20-Poly1305 at rest
    │
    └── Auth tokens
        └── VerifyTokenEqual() ← Constant-time comparison
```

## CryptoError (lib.rs:18)

```go
var (
    ErrEncryptionFailed    = errors.New("encryption failed")
    ErrDecryptionFailed    = errors.New("decryption failed")
    ErrKeyDerivationFailed = errors.New("key derivation failed")
    ErrHashingFailed       = errors.New("hashing failed")
    ErrVerificationFailed  = errors.New("verification failed")
    ErrInvalidKeyLength    = errors.New("invalid key length")
)
```

**Important:** Never leak key material in error messages.

## Go Implementation Notes

1. **ChaCha20-Poly1305:** `golang.org/x/crypto/chacha20poly1305` - same algorithm
2. **Argon2id:** `golang.org/x/crypto/argon2` - use `argon2.IDKey()`
3. **HMAC-SHA256:** `crypto/hmac` + `crypto/sha256` - stdlib
4. **Constant-time compare:** `crypto/subtle.ConstantTimeCompare()`
5. **Zeroize on drop:** Go doesn't have destructors. Use `runtime.SetFinalizer()` or explicit `Clear()` methods for sensitive keys. Consider wrapping in a struct with a `Destroy()` method.
6. **Unicode sanitization:** `unicode.IsControl()` + `unicode.Is(unicode.Cf, r)` cover the same chars
7. **SSRF:** `net.LookupIP()` then check each resolved IP - same pattern
8. **File permissions:** Use `os.OpenFile()` with `0600` for sensitive files
