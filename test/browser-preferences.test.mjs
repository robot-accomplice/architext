import assert from "node:assert/strict";
import test from "node:test";
import {
  readBooleanPreference,
  readDebugRouting,
  readRoutingStylePreference,
  writeBooleanPreference,
  writeRoutingStylePreference
} from "../docs/architext/src/adapters/browserPreferences.js";

function memoryStorage(values = {}) {
  const store = new Map(Object.entries(values));
  return {
    getItem: (key) => store.has(key) ? store.get(key) : null,
    setItem: (key, value) => store.set(key, value)
  };
}

test("browser preferences normalize legacy curved routing to spline", () => {
  assert.equal(readRoutingStylePreference(memoryStorage({ "architext-routing-style": "curved" })), "spline");
  assert.equal(readRoutingStylePreference(memoryStorage({ "architext-routing-style": "spline" })), "spline");
  assert.equal(readRoutingStylePreference(memoryStorage({ "architext-routing-style": "straight" })), "straight");
  assert.equal(readRoutingStylePreference(memoryStorage({ "architext-routing-style": "unknown" })), "orthogonal");
});

test("browser preferences read and write persisted values", () => {
  const storage = memoryStorage();

  assert.equal(readBooleanPreference(storage, "collapsed"), false);
  writeBooleanPreference(storage, "collapsed", true);
  assert.equal(readBooleanPreference(storage, "collapsed"), true);

  writeRoutingStylePreference(storage, "straight");
  assert.equal(readRoutingStylePreference(storage), "straight");
});

test("browser preferences read debug routing from query string", () => {
  assert.equal(readDebugRouting("?debugRouting=1"), true);
  assert.equal(readDebugRouting("?debugRouting=0"), false);
  assert.equal(readDebugRouting(""), false);
});
