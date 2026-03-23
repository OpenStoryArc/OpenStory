# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Open Story, please report it privately:

Use [GitHub Private Vulnerability Reporting](https://github.com/OpenStoryArc/OpenStory/security/advisories/new) to report privately.

Please do **not** open a public issue for security vulnerabilities.

## What to include

- Description of the vulnerability
- Steps to reproduce
- Affected versions or components
- Potential impact

## Response timeline

- **Acknowledgment**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Fix or mitigation**: Depends on severity, but we aim for patches within 2 weeks for critical issues

## Scope

Open Story runs locally and observes agent activity. Security concerns include:

- **Data exposure**: Event data, session transcripts, and configuration files
- **Injection**: SQL injection via SQLite, command injection via tool inputs
- **Authentication bypass**: API token validation, CORS policy
- **Dependency vulnerabilities**: Tracked via Dependabot

Out of scope: Issues that require physical access to the machine where Open Story is running.
