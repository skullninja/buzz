import assert from "node:assert/strict";
import { test } from "node:test";
import { threadReplyRecipients } from "./threadReplyRecipients.ts";

const SELF = "a".repeat(64);
const ALICE = "b".repeat(64);
const BOB = "c".repeat(64);

test("adds the parent author so a threaded reply reaches them", () => {
  assert.deepEqual(threadReplyRecipients([], ALICE, SELF), [ALICE]);
});

test("keeps explicit mentions and appends the parent author", () => {
  assert.deepEqual(threadReplyRecipients([BOB], ALICE, SELF), [BOB, ALICE]);
});

test("does not duplicate an author who is already mentioned", () => {
  assert.deepEqual(threadReplyRecipients([ALICE], ALICE, SELF), [ALICE]);
});

test("never notifies yourself when replying to your own message", () => {
  assert.deepEqual(threadReplyRecipients([], SELF, SELF), []);
});

test("an unknown parent author is a no-op, not a failure", () => {
  // The parent may not be in cache. A reply must still send.
  assert.deepEqual(threadReplyRecipients([BOB], undefined, SELF), [BOB]);
});
