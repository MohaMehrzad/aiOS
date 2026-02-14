# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

Please report security vulnerabilities using [GitHub's private vulnerability reporting](https://github.com/MohaMehrzad/aiOS/security/advisories/new) on this repository.

We will:
- Acknowledge receipt within **48 hours**
- Provide a detailed response within **7 days**
- Include next steps and an expected timeline for a fix

## Security Measures

aiOS takes security seriously as an operating system project:

- **Automated dependency scanning** via Dependabot (Cargo, pip, GitHub Actions)
- **Static analysis** via CodeQL, cargo-audit, and clippy
- **Secret scanning** with push protection enabled
- **Mandatory code review** for all changes to `main`
- **CI checks** must pass before merging (Rust build/test/lint + Python lint/test)
- **AppArmor profiles** for all agent processes at runtime
- **Capability-based access control** — tools declare required capabilities, agents must possess them
- **Hash-chained audit ledger** for every tool execution
- **Sandboxing** via Podman rootless containers for untrusted operations
