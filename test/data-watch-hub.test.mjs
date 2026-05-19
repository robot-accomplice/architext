import assert from "node:assert/strict";
import test from "node:test";
import { EventEmitter } from "node:events";
import { createDataWatchHub } from "../src/adapters/http/data-watch-hub.mjs";

class ResponseStub extends EventEmitter {
  constructor() {
    super();
    this.body = "";
    this.headers = null;
  }

  writeHead(status, headers) {
    this.status = status;
    this.headers = headers;
  }

  write(value) {
    this.body += value;
  }

  end() {
    this.ended = true;
  }
}

test("data watch hub debounces json writes and broadcasts validated refresh events", async () => {
  let timerCallback = null;
  let validateCount = 0;
  const response = new ResponseStub();
  const hub = createDataWatchHub({
    target: "/tmp/repo",
    dataDir: (target) => `${target}/docs/architext/data`,
    validateTarget: async () => {
      validateCount += 1;
      return { ok: true, output: "valid" };
    },
    watchFn: () => ({ close() {} }),
    setTimer: (callback) => {
      timerCallback = callback;
      return callback;
    },
    clearTimer: () => {
      timerCallback = null;
    }
  });

  hub.attach(response);
  hub.schedule("flows.json");
  hub.schedule("views.json");
  await timerCallback();

  assert.equal(validateCount, 1);
  assert.match(response.body, /"type":"valid"/);
  assert.match(response.body, /"version":1/);
});

test("data watch hub ignores non-json writes and reports invalid validation state", async () => {
  let timerCallback = null;
  const response = new ResponseStub();
  const hub = createDataWatchHub({
    target: "/tmp/repo",
    dataDir: (target) => `${target}/docs/architext/data`,
    validateTarget: async () => ({ ok: false, output: "schema failed" }),
    watchFn: () => ({ close() {} }),
    setTimer: (callback) => {
      timerCallback = callback;
      return callback;
    },
    clearTimer: () => {
      timerCallback = null;
    }
  });

  hub.attach(response);
  hub.schedule("notes.md");
  assert.equal(timerCallback, null);

  hub.schedule("manifest.json");
  await timerCallback();
  assert.match(response.body, /"type":"invalid"/);
  assert.match(response.body, /schema failed/);
});
