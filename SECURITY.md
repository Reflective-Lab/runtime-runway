# Security Policy

This policy covers vulnerability reporting for the Reflective Runway repository.

## Supported Versions

We provide security updates for the following versions:

| Version | Supported          |
|---------|--------------------|
| 3.x     | :white_check_mark: |
| < 3.0   | :x:                |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Report vulnerabilities through [GitHub Security Advisories](https://github.com/Reflective-Lab/runway/security/advisories/new)
or by emailing **Kenneth Pernyer** at [kenneth@reflective.se](mailto:kenneth@reflective.se).

You should receive a response within 48 hours. If for some reason you do not, please follow up via email to ensure we received your original message.

Please include the following information in your report:

- The version of Runway you're using
- A description of the vulnerability
- Steps to reproduce the issue
- Any relevant logs or error messages
- Your assessment of the impact (CVSS score if possible)

## Security Update Process

1. **Acknowledgment**: We will acknowledge your report within 48 hours
2. **Assessment**: We will assess the vulnerability and determine its impact
3. **Patch Development**: We will develop a fix and test it thoroughly
4. **Release**: We will release the fix in a new version
5. **Disclosure**: We will publicly disclose the vulnerability after the fix is available

## Built-in Security Practices

- `unsafe_code = "forbid"` across all crates
- Dependency auditing via `cargo-deny` (when available)
- Clippy pedantic lints enforced in CI
- No secrets in source — deployment scripts read from environment or Secret Manager
- Container images use minimal base images (`debian:bookworm-slim`)
- GPU workers run without unauthenticated access on Cloud Run

## Shared Responsibility

This repository provides deployment tooling and distribution binaries. Production security depends on deployment-specific controls.

Deployers are responsible for:

- Infrastructure hardening and patching
- Identity provider and access control configuration
- Encryption key management and rotation
- Network security and firewall rules
- Model artifact integrity and provenance
- Monitoring and alerting

## Security Best Practices

When deploying with Runway:

- Keep dependencies updated
- Use the latest stable version
- Never commit `.env` files or credentials
- Use Secret Manager for sensitive configuration
- Enable TLS for all gRPC endpoints
- Restrict GPU worker access to authenticated clients
- Monitor Cloud Run logs for anomalies

## Security Contact

For security-related questions or concerns:

**Kenneth Pernyer**
- Email: [kenneth@reflective.se](mailto:kenneth@reflective.se)
- PGP Key: Available upon request

## Responsible Disclosure

We ask security researchers to:

- Give us reasonable time to respond before making issues public
- Avoid exploiting vulnerabilities in production systems
- Avoid violating privacy laws or disrupting services
- Provide sufficient detail to reproduce the issue

We commit to:

- Responding promptly to security reports
- Providing regular updates on our progress
- Crediting reporters in our security advisories (unless anonymous)
- Releasing fixes in a timely manner
