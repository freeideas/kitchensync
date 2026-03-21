# URL Normalization

Rules for normalizing URLs before storage and lookup.

## $REQ_NORM_001: Lowercase Scheme and Hostname
**Source:** ./specs/database.md (Section: "URL Normalization")

The scheme and hostname are lowercased during normalization.

## $REQ_NORM_002: Remove Default Port
**Source:** ./specs/database.md (Section: "URL Normalization")

The default port (22 for SFTP) is removed during normalization.

## $REQ_NORM_003: Collapse Consecutive Slashes
**Source:** ./specs/database.md (Section: "URL Normalization")

Consecutive slashes in the path are collapsed during normalization.

## $REQ_NORM_004: Remove Trailing Slash
**Source:** ./specs/database.md (Section: "URL Normalization")

Trailing slashes are removed from the path during normalization.

## $REQ_NORM_005: Bare Path to File URL
**Source:** ./specs/database.md (Section: "URL Normalization")

Bare paths (no scheme) are converted to `file://` URLs during normalization.

## $REQ_NORM_006: File URL Absolute Resolution
**Source:** ./specs/database.md (Section: "URL Normalization")

`file://` URLs are resolved to absolute paths from the current working directory during normalization.

## $REQ_NORM_007: Percent-Decode Unreserved Characters
**Source:** ./specs/database.md (Section: "URL Normalization")

Percent-encoded unreserved characters are decoded during normalization.

