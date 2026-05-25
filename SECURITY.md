# Security Policy

## Threat Model

Bua is designed to execute untrusted or semi-trusted JavaScript agents in a capability-secure sandbox. The security model assumes:

**Trusted:** The Bua runtime binary, the host OS, the operator who sets capability flags.

**Untrusted:** The JavaScript code being executed, external tool responses, module sources.

### Security Boundaries

| Boundary | Mechanism |
|----------|-----------|
| Filesystem access | `FsCapability` with path prefix matching |
| Network access | `NetCapability` with host glob matching |
| Subprocess execution | `SubprocessCapability` with allowlist |
| Environment variables | `EnvCapability` with key allowlist |
| Agent spawning | `AgentSpawn` capability + child ⊆ parent rule |
| Cross-agent data | Explicit IPC only — no shared memory |

### Capability Invariants

1. **Deny by default** — No capability = no access. Always.
2. **Non-escalatable** — Child agents cannot receive capabilities their parent doesn't have.
3. **Revocable** — Any capability can be revoked mid-execution. The generation counter ensures stale checks are detected.
4. **Auditable** — Every permission check (allowed or denied) is logged to the execution trace.

### JS Engine Isolation

Each agent runs in its own JSC context with its own heap. Agents cannot access each other's JS state. Cross-agent communication requires explicit tool calls.

### Reentrancy Protection

Bua enforces a strict no-reentrancy rule: JSC is never called while an eval is in progress. All async results queue in `ResolutionQueue` and drain at safe points. This prevents:
- Stack corruption from reentrant JSC calls
- Promise ordering violations
- GC hazards from concurrent heap access

## Reporting Vulnerabilities

**Please do not report security vulnerabilities via public GitHub issues.**

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

We will acknowledge receipt within 48 hours and aim to release a fix within 14 days for critical issues.

## Known Limitations (Current Alpha)

- JSC heap isolation relies on JSC's own GC. JSC-level exploits (e.g., JIT bugs) are out of scope for Bua's security model.
- Snapshot deserialization uses serde_json — malformed snapshots are rejected via CRC32, but deeply nested JSON structures may cause stack overflow (fix: bounded recursion limit).
- `--allow-all` disables all capability checks. Only use for fully trusted scripts in isolated environments.
- Path traversal in `FsCapability`: Bua uses `starts_with()` for path matching. Symlinks are not followed by default but callers should canonicalize paths before granting capabilities.
