# Homebrew distribution

`architext.rb` is the Homebrew formula for the native Architext binary. It
installs the per-platform release binary directly (macOS arm64/x64, Linux
arm64/x64) — no npm, no Node. Windows is not a Homebrew target; Windows users use
the native installer or `architext update`.

## Publishing the tap (one-time)

1. Create a repo named `robot-accomplice/homebrew-architext` (the
   `homebrew-` prefix is what makes `brew tap robot-accomplice/architext` work).
2. Copy this formula to `Formula/architext.rb` in that repo and push.

Users then install with:

```sh
brew install robot-accomplice/architext/architext
# or
brew tap robot-accomplice/architext && brew install architext
```

## Updating on each release

Bump `version` in the formula and replace the four `sha256` values with the
digests for the new release. The digests are published as the release's
`SHA256SUMS` asset:

```sh
ver=1.7.3
curl -fsSL "https://github.com/robot-accomplice/architext/releases/download/v${ver}/SHA256SUMS"
```

Map each line to the matching `url`: `architext-darwin-arm64`,
`architext-darwin-x64`, `architext-linux-arm64`, `architext-linux-x64`. The `url`
fields already interpolate `#{version}`, so only `version` + the `sha256`s change.

A future enhancement is to have the release pipeline regenerate this formula and
push it to the tap automatically (the same shape as the roboticus-site release
dispatch), so the tap never goes stale.
