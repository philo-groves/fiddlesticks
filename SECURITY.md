# Security Policy

## Supported Versions

The project currently tracks security fixes on the latest released line, and the prerelease line.

## Reporting a Vulnerability (GitHub Issues)

Security reports are tracked in GitHub Issues.

1. Search existing issues first (open and closed) for `label:security`.
2. If no match exists, open a new issue with a clear title prefix, for example: `[security] short description`.
3. Add the `security` label (or request it in the issue body if you do not have label permissions).
4. Include:
   - affected crate(s) and version(s)
   - impact and attack scenario
   - reproduction steps or proof of concept
   - suggested mitigation or patch direction (if known)

Do not post secrets, tokens, credentials, or private infrastructure details in public issues.

## Triage and Tracking

Security issues are tracked using standard GitHub issue workflow:

- **Labels**: `security`, `triage`, `needs-info`, `confirmed`, `in-progress`, `released`
- **State flow**: `Open` -> `Triaged` -> `In Progress` -> `Released` -> `Closed`
- **Ownership**: a maintainer is assigned during triage
- **Milestones**: issues are attached to the target patch/minor release milestone

Response targets:

- Initial triage acknowledgment: within 3 business days
- Severity assessment and plan: within 7 business days
- Regular status updates: at least weekly while open

## Severity Guidance

Severity is assessed using a CVSS-style approach and practical impact on:

- confidentiality
- integrity
- availability
- exploitability in default deployment paths

## Disclosure Process

After a fix is merged and released:

1. The issue is updated with impacted versions, fixed version, and upgrade guidance.
2. Release notes/changelog include a security entry.
3. The issue is closed with references to the fixing commit(s) and release tag.
