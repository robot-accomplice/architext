# Package Update Version Precedence

`architext --check-updates` compares the installed package version with the
latest npm version before prompting for a global install. That comparison must
follow semantic-version precedence, including prerelease identifiers.

## Contract

- Leading `v` is accepted for local or registry versions.
- Build metadata is ignored for precedence.
- A stable release is newer than a prerelease with the same core version.
- Prerelease identifiers compare segment by segment.
- Numeric prerelease identifiers sort numerically and before non-numeric
  identifiers.

Malformed versions are outside the update-check contract. The package and npm
registry provide semantic versions.

## Verification

- `1.4.5` sorts after `1.4.5-beta.2`.
- `1.4.5-beta.10` sorts after `1.4.5-beta.2`.
- Build metadata does not affect equality.
