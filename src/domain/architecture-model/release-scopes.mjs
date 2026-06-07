export function releaseItems(detail) {
  if (!detail) return [];
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

export function releaseScopeEntries(scope) {
  return [
    ["required", scope.required],
    ["planned", scope.planned],
    ["stretch", scope.stretch],
    ["deferred", scope.deferred],
    ["outOfScope", scope.outOfScope]
  ];
}

