---
name: review-unsafe
description: Audit and review unsafe Rust code blocks in psmux, especially Win32 FFI calls. Use when the user asks to "review unsafe", "audit safety", "check FFI", "review Win32 calls", "security review", or when working on code that involves unsafe blocks, raw pointers, or Windows API interop.
---

# Reviewing Unsafe Code in psmux

psmux necessarily uses `unsafe` for Windows API interop. Every unsafe block must be justified and minimal.

## Audit Checklist for Each `unsafe` Block

1. **SAFETY comment present?**
   Every `unsafe` block MUST have a `// SAFETY:` comment explaining why it's sound.

2. **Minimal scope?**
   The unsafe block should contain only the FFI call, not surrounding logic.

   ```rust
   // BAD: too much in unsafe
   unsafe {
       let handle = GetStdHandle(STD_OUTPUT_HANDLE);
       let mut mode = 0u32;
       GetConsoleMode(handle, &mut mode);
       mode |= ENABLE_VIRTUAL_TERMINAL_PROCESSING;
       SetConsoleMode(handle, mode);
   }

   // GOOD: each call isolated or grouped logically
   // SAFETY: GetStdHandle returns a valid handle or INVALID_HANDLE_VALUE,
   // which we check immediately after.
   let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
   assert!(handle != INVALID_HANDLE_VALUE);
   ```

3. **Return value checked?**
   Win32 functions return `BOOL` (0 = failure) or `HANDLE` (`INVALID_HANDLE_VALUE` / null = failure). Always check.

4. **Null pointer derefs impossible?**
   If a raw pointer is returned, verify it's non-null before dereferencing.

5. **Buffer sizes correct?**
   Functions like `ReadConsoleInput`, `WriteConsoleOutput` take buffer + size. Ensure the size matches the actual buffer allocation.

6. **Lifetime correctness?**
   Handles from `CreateFile`, `CreatePseudoConsole`, etc. must be closed. Use RAII wrappers (a struct with `Drop` impl that calls `CloseHandle`).

7. **Thread safety?**
   Console handles can be shared across threads. Ensure proper synchronization if accessed from multiple threads.

## Common Win32 APIs in psmux to Audit

| API | Risk | What to verify |
|-----|------|----------------|
| `CreatePseudoConsole` | Handle leak | Closed via `ClosePseudoConsole` |
| `CreateNamedPipeW` | Access control | Security descriptor set correctly |
| `ReadConsoleInput` | Buffer overflow | Buffer size matches `nLength` param |
| `WriteConsoleOutput` | Buffer overflow | Region and buffer dimensions agree |
| `SetConsoleMode` | State corruption | Original mode saved and restorable |
| `GetConsoleScreenBufferInfo` | Null deref | Handle validity checked |

## Running the Audit

```bash
# Find all unsafe blocks
grep -rn "unsafe" src/ --include="*.rs"

# Check for missing SAFETY comments
grep -B1 "unsafe {" src/ --include="*.rs" | grep -v "SAFETY"

# Clippy's unsafe lints
cargo clippy -- -W clippy::undocumented_unsafe_blocks -D warnings
```

## Safe Abstraction Pattern
Wrap FFI in safe Rust types:

```rust
struct ConsoleHandle(HANDLE);

impl ConsoleHandle {
    fn stdout() -> Result<Self> {
        // SAFETY: GetStdHandle with STD_OUTPUT_HANDLE always returns
        // a valid handle or INVALID_HANDLE_VALUE.
        let h = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if h == INVALID_HANDLE_VALUE {
            return Err(anyhow!("Failed to get stdout handle"));
        }
        Ok(Self(h))
    }
}

impl Drop for ConsoleHandle {
    fn drop(&mut self) {
        // SAFETY: self.0 is guaranteed valid by the constructor check.
        // Stdout handle should not be closed, so only close for
        // handles we own (pipes, pseudo-consoles, etc.)
        if self.is_owned {
            unsafe { CloseHandle(self.0); }
        }
    }
}
```
