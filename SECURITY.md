# Security policy

## Reporting a vulnerability

Please open a private security advisory through GitHub instead of a public issue. Include the affected version, a minimal reproduction, and the expected impact.

## Credential handling

OAuth credentials are encrypted with Windows DPAPI for the current user. Plaintext credential files should be deleted immediately after running `protect-credentials` and must never be committed.

The repository intentionally ignores credential files, databases, logs, and environment files. Releases contain only the compiled executable.
