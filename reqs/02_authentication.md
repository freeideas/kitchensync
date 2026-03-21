# Authentication

SSH authentication fallback chain and host key verification for SFTP connections.

## $REQ_AUTH_001: Inline Password
**Source:** ./README.md (Section: "Authentication (fallback chain)")

The first authentication method tried is the inline password from the URL.

## $REQ_AUTH_002: SSH Agent
**Source:** ./README.md (Section: "Authentication (fallback chain)")

The second authentication method tried is the SSH agent (`SSH_AUTH_SOCK`).

## $REQ_AUTH_003: SSH Key ed25519
**Source:** ./README.md (Section: "Authentication (fallback chain)")

The third authentication method tried is `~/.ssh/id_ed25519`.

## $REQ_AUTH_004: SSH Key ECDSA
**Source:** ./README.md (Section: "Authentication (fallback chain)")

The fourth authentication method tried is `~/.ssh/id_ecdsa`.

## $REQ_AUTH_005: SSH Key RSA
**Source:** ./README.md (Section: "Authentication (fallback chain)")

The fifth authentication method tried is `~/.ssh/id_rsa`.

## $REQ_AUTH_006: Host Key Verification
**Source:** ./README.md (Section: "Authentication (fallback chain)")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.

## $REQ_AUTH_007: Password Percent Encoding
**Source:** ./README.md (Section: "URL schemes")

Special characters in SFTP passwords must be percent-encoded (`@` → `%40`, `:` → `%3A`).
