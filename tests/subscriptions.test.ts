import assert from "node:assert/strict";
import test from "node:test";
import { createSubscriptionScope } from "../src/subscriptions.ts";

test("late subscriptions are disposed after the effect unmounts", async () => {
  let resolve!: (cleanup: () => void) => void;
  let cleanups = 0;
  const scope = createSubscriptionScope(assert.fail);
  const pending = scope.add(new Promise((done) => { resolve = done; }));
  scope.dispose();
  resolve(() => { cleanups++; });
  await pending;
  scope.dispose();
  assert.equal(cleanups, 1);
});

test("StrictMode effect instances own separate subscriptions", async () => {
  let firstCleanups = 0;
  let secondCleanups = 0;
  const first = createSubscriptionScope(assert.fail);
  const second = createSubscriptionScope(assert.fail);
  await first.add(Promise.resolve(() => { firstCleanups++; }));
  await second.add(Promise.resolve(() => { secondCleanups++; }));
  first.dispose();
  first.dispose();
  assert.equal(firstCleanups, 1);
  assert.equal(secondCleanups, 0);
  second.dispose();
  assert.equal(secondCleanups, 1);
});

test("registration failures are handled without leaking prior listeners", async () => {
  const errors: unknown[] = [];
  let cleaned = false;
  const scope = createSubscriptionScope((error) => errors.push(error));
  await scope.add(Promise.resolve(() => { cleaned = true; }));
  await scope.add(Promise.reject("failed"));
  scope.dispose();
  await scope.add(Promise.reject("after unmount"));
  assert.deepEqual(errors, ["failed"]);
  assert.ok(cleaned);
});

test("one failing cleanup does not prevent the remaining cleanups", async () => {
  const errors: unknown[] = [];
  let secondCleaned = false;
  const scope = createSubscriptionScope((error) => errors.push(error));
  await scope.add(Promise.resolve(() => { throw new Error("cleanup failed"); }));
  await scope.add(Promise.resolve(() => { secondCleaned = true; }));
  scope.dispose();
  assert.equal(errors.length, 1);
  assert.ok(secondCleaned);
});
