# Code Review Checklist

Use this checklist when reviewing PRs, MRs, and Gerrit changes. Items are ordered by priority.

## Security

- [ ] **Injection**: SQL, command, XSS, template injection in user-facing inputs
- [ ] **Authentication/Authorization**: Missing or bypassed auth checks, privilege escalation paths
- [ ] **Data exposure**: Sensitive data in logs, error messages, API responses, or client-side code
- [ ] **Secrets**: Hardcoded credentials, API keys, tokens, or connection strings
- [ ] **Deserialization**: Unsafe deserialization of untrusted data (pickle, yaml.load, eval)
- [ ] **Path traversal**: User-controlled file paths without sanitization
- [ ] **SSRF**: Server-side requests with user-controlled URLs
- [ ] **Cryptography**: Weak algorithms, custom crypto, missing encryption for sensitive data

## Maintainability

- [ ] **Complexity**: Functions/methods exceeding reasonable cognitive complexity
- [ ] **Naming**: Variables, functions, or classes with unclear or misleading names
- [ ] **Separation of concerns**: Business logic mixed with I/O, presentation, or infrastructure
- [ ] **Code duplication**: Copy-pasted logic that should be extracted (only if 3+ occurrences or high change risk)
- [ ] **Dead code**: Unreachable code, unused imports, commented-out blocks left behind
- [ ] **Documentation**: Missing documentation for public APIs or non-obvious behavior

## Coding Practices

- [ ] **Error handling**: Missing error handling at system boundaries, swallowed exceptions
- [ ] **Resource management**: Unclosed files, connections, or handles; missing cleanup
- [ ] **Race conditions**: Shared mutable state without synchronization, TOCTOU issues
- [ ] **Input validation**: Missing validation at system boundaries (user input, external APIs)
- [ ] **Null/undefined**: Unguarded access to potentially null/undefined values
- [ ] **Boundary conditions**: Off-by-one errors, empty collections, integer overflow
- [ ] **Logging**: Missing audit logging for security-relevant operations

## Architecture

- [ ] **Pattern consistency**: Does the change follow existing codebase patterns and conventions?
- [ ] **Abstraction level**: Is the abstraction appropriate -- not too high (over-engineered) or too low (procedural soup)?
- [ ] **Dependency direction**: Do dependencies point in the right direction? No circular dependencies?
- [ ] **API design**: Are new public APIs consistent with existing ones? Will they be stable?
- [ ] **Configuration**: Are new configurable values appropriately externalized?
- [ ] **Backwards compatibility**: Does the change break existing consumers or contracts?

## What NOT to Flag

- Style/formatting issues covered by linters (whitespace, import order, line length)
- Minor naming preferences without clear readability impact
- Test coverage gaps (leave to CI coverage tools)
- Issues already caught by failing CI checks
- Hypothetical future concerns ("what if someone later...")
