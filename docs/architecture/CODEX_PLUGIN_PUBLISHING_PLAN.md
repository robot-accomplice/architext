# Codex Plugin Publishing Plan

Architext should publish one Codex plugin from the repository root. The plugin
should expose the existing `skills/` directory rather than copying skill files
into a nested plugin folder.

## Architecture

The repository root is the plugin root:

- `.codex-plugin/plugin.json` is the Codex plugin manifest.
- `skills/architext/SKILL.md` is the single Architext skill source.
- `skills/architext/agents/openai.yaml` supplies Codex UI metadata for the skill.
- `package.json` includes `.codex-plugin` and `skills` in the npm package files.

This keeps npm package publishing and plugin publishing aligned around one
source tree. A generated nested plugin under `plugins/architext` would duplicate
the skill and create drift between the package, repository documentation, and
plugin distribution.

## Manifest Contract

The Codex plugin manifest should stay minimal:

- name: `architext`
- version: match the package release version
- skills: `./skills/`
- no apps or MCP server declarations until those files exist
- no unsupported fields that fail plugin validation

Marketplace metadata is a distribution concern, not the core plugin contract.
For a local personal marketplace, Codex currently expects plugin entries under
`./plugins/<name>` relative to the marketplace root. That is useful for local
installation, but it should not drive this repository into a nested plugin
layout.

## Acceptance Criteria

- The existing Architext skill validates as a Codex skill.
- The repository root validates as a Codex plugin.
- The npm package includes `.codex-plugin` and `skills`.
- Plugin metadata mirrors the package identity and repository ownership.
- Any future marketplace entry points to the published plugin artifact or a
  deliberate local checkout path, not a second copy of the skill.
