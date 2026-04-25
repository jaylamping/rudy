import assert from "node:assert/strict";
import { formatRunFailure } from "./ssh.js";

const fake = {
  code: 1,
  signal: null,
  stdout: "out",
  stderr: "err",
  truncated: false,
  timedOut: false,
};

assert.ok(formatRunFailure("t", fake).includes("out"));
assert.ok(formatRunFailure("t", fake).includes("err"));

console.log("ssh.test: ok");
