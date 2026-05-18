export function readBooleanPreference(storage, key, defaultValue = false) {
  const value = storage?.getItem?.(key);
  if (value === null || value === undefined) return defaultValue;
  return value === "true";
}

export function writeBooleanPreference(storage, key, value) {
  storage?.setItem?.(key, String(value));
}

export function readRoutingStylePreference(storage) {
  const stored = storage?.getItem?.("architext-routing-style");
  if (stored === "straight") return "straight";
  return stored === "spline" || stored === "curved" ? "spline" : "orthogonal";
}

export function writeRoutingStylePreference(storage, value) {
  storage?.setItem?.("architext-routing-style", value);
}

export function readDebugRouting(search) {
  return new URLSearchParams(search).get("debugRouting") === "1";
}
