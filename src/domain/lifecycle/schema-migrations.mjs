function parseVersion(version) {
  const match = String(version ?? "").match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match) return null;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3])
  };
}

function compareVersions(left, right) {
  const parsedLeft = parseVersion(left);
  const parsedRight = parseVersion(right);
  if (!parsedLeft || !parsedRight) return String(left ?? "").localeCompare(String(right ?? ""));
  for (const key of ["major", "minor", "patch"]) {
    if (parsedLeft[key] !== parsedRight[key]) return parsedLeft[key] - parsedRight[key];
  }
  return 0;
}

function migrationKind(fromVersion, toVersion) {
  const from = parseVersion(fromVersion);
  const to = parseVersion(toVersion);
  if (!from || !to) return "unknown";
  if (to.major > from.major) return "breaking";
  return "additive";
}

export function schemaMigrationPlan({ currentVersion, targetVersion }) {
  const current = currentVersion || "";
  const target = targetVersion || "";
  if (!target || current === target) {
    return {
      currentVersion: current,
      targetVersion: target,
      pending: [],
      upToDate: true
    };
  }

  const direction = compareVersions(current, target);
  if (direction > 0) {
    return {
      currentVersion: current,
      targetVersion: target,
      pending: [{
        id: "schema-version-ahead",
        kind: "unsupported",
        file: "docs/architext/data/manifest.json",
        fromVersion: current,
        toVersion: target,
        summary: `target schemaVersion ${current} is newer than CLI schema ${target}; install a newer Architext CLI before migrating`
      }],
      upToDate: false
    };
  }

  const fromLabel = current || "missing";
  const kind = migrationKind(current, target);
  return {
    currentVersion: current,
    targetVersion: target,
    pending: [{
      id: `schema-version-${fromLabel}-to-${target}`,
      kind,
      file: "docs/architext/data/manifest.json",
      fromVersion: current,
      toVersion: target,
      summary: `apply ${kind} schema migration ${fromLabel} -> ${target}: update manifest.schemaVersion`
    }],
    upToDate: false
  };
}
