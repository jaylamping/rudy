import assert from "node:assert/strict";
import {
  healthMaxWaitSec,
  healthWaitMs,
  logLines,
  sanitizeSince,
} from "./validation.js";

assert.equal(logLines(undefined), 200);
assert.equal(logLines(5000), 2000);
assert.equal(logLines(10), 10);

assert.equal(sanitizeSince("10 min ago"), "10 min ago");
assert.equal(sanitizeSince("; rm -rf /"), undefined);
assert.equal(sanitizeSince("a".repeat(100)), undefined);

assert.equal(healthWaitMs(undefined), 30_000);
assert.equal(healthMaxWaitSec(5000), 5);
assert.equal(healthMaxWaitSec(400_000), 300);

console.log("validation.test: ok");
