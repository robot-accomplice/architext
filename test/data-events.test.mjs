import assert from "node:assert/strict";
import test from "node:test";
import { subscribeToDataEvents } from "../docs/architext/src/adapters/dataEvents.js";

test("data event adapter routes valid and invalid server events", () => {
  const events = [];
  class EventSourceStub {
    constructor(url) {
      this.url = url;
      EventSourceStub.instance = this;
    }

    close() {
      this.closed = true;
    }
  }

  const unsubscribe = subscribeToDataEvents({
    EventSourceCtor: EventSourceStub,
    onValid: (payload) => events.push(["valid", payload.version]),
    onInvalid: (payload) => events.push(["invalid", payload.output])
  });

  assert.equal(EventSourceStub.instance.url, "/api/data-events");
  EventSourceStub.instance.onmessage({ data: JSON.stringify({ type: "valid", version: 1 }) });
  EventSourceStub.instance.onmessage({ data: JSON.stringify({ type: "invalid", output: "bad json" }) });
  unsubscribe();

  assert.deepEqual(events, [["valid", 1], ["invalid", "bad json"]]);
  assert.equal(EventSourceStub.instance.closed, true);
});
