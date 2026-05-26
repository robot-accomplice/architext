# Serve Path Decoding

The package-owned viewer server treats request paths as untrusted input. Path
decoding is part of request validation, not an internal server error.

## Contract

- Malformed percent-encoding never escapes the static file boundary as a 500.
- Invalid decoded paths resolve to no file.
- Data file requests for invalid paths return `404 Not found`.
- Static asset fallback remains constrained to the packaged viewer directory.

The path join helper owns decoding and root containment so callers do not need
to know which malformed input cases can throw.

## Verification

- A malformed `/data/...` path returns 404 and does not expose decoder errors.
