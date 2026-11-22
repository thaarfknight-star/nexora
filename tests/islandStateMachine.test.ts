// State-machine transition tests. Run with `npm test`
// (node:test — built into Node, no dependency). This file is outside
// tsconfig's `include: ["src"]`, so tsc does not check it; Node's
// type stripping runs it as-is.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  nextVisible,
  shouldArmAutoClose,
} from "../src/islandStateMachine.ts";
import type {
  IslandGuards,
  IslandVisible,
} from "../src/islandStateMachine.ts";

const open: IslandGuards = {
  onboardingOpen: false,
  pinned: false,
  hovering: false,
  launchHintActive: false,
};

const withGuards = (patch: Partial<IslandGuards>): IslandGuards => ({
  ...open,
  ...patch,
});

const states: IslandVisible[] = ["hidden", "compact", "expanded"];

test("hot-zone-enter wakes hidden to compact, and nothing else", () => {
  assert.equal(nextVisible("hidden", "hot-zone-enter", open), "compact");
  assert.equal(nextVisible("compact", "hot-zone-enter", open), "compact");
  assert.equal(nextVisible("expanded", "hot-zone-enter", open), "expanded");
});

test("hot-zone-leave changes nothing by itself", () => {
  for (const state of states) {
    assert.equal(nextVisible(state, "hot-zone-leave", open), state);
  }
});

test("auto-close-timeout steps one level down, unconditionally", () => {
  assert.equal(nextVisible("expanded", "auto-close-timeout", open), "compact");
  assert.equal(nextVisible("compact", "auto-close-timeout", open), "hidden");
  assert.equal(nextVisible("hidden", "auto-close-timeout", open), "hidden");
  // Guards are checked at arm time, not when the timer fires.
  const pinned = withGuards({ pinned: true });
  assert.equal(nextVisible("expanded", "auto-close-timeout", pinned), "compact");
});

test("esc steps down, but never when pinned or onboarding", () => {
  assert.equal(nextVisible("expanded", "esc", open), "compact");
  assert.equal(nextVisible("compact", "esc", open), "hidden");
  assert.equal(nextVisible("hidden", "esc", open), "hidden");
  assert.equal(
    nextVisible("expanded", "esc", withGuards({ pinned: true })),
    "expanded",
  );
  assert.equal(
    nextVisible("compact", "esc", withGuards({ onboardingOpen: true })),
    "compact",
  );
});

test("hotkey toggles, ignores pin, never dismisses onboarding", () => {
  assert.equal(nextVisible("hidden", "hotkey", open), "compact");
  assert.equal(nextVisible("compact", "hotkey", open), "hidden");
  assert.equal(nextVisible("expanded", "hotkey", open), "hidden");
  assert.equal(
    nextVisible("expanded", "hotkey", withGuards({ pinned: true })),
    "hidden",
  );
  assert.equal(
    nextVisible("expanded", "hotkey", withGuards({ onboardingOpen: true })),
    "expanded",
  );
});

test("shell-click steps down, never dismisses onboarding or a pinned island", () => {
  assert.equal(nextVisible("expanded", "shell-click", open), "compact");
  assert.equal(nextVisible("compact", "shell-click", open), "hidden");
  assert.equal(
    nextVisible("expanded", "shell-click", withGuards({ onboardingOpen: true })),
    "expanded",
  );
  // "Keep open" means it stays open: like Esc, a shell click does
  // not collapse a pinned island.
  assert.equal(
    nextVisible("expanded", "shell-click", withGuards({ pinned: true })),
    "expanded",
  );
  assert.equal(
    nextVisible("compact", "shell-click", withGuards({ pinned: true })),
    "compact",
  );
});

test("toggle-expand flips compact/expanded and never touches hidden", () => {
  assert.equal(nextVisible("compact", "toggle-expand", open), "expanded");
  assert.equal(nextVisible("expanded", "toggle-expand", open), "compact");
  assert.equal(nextVisible("hidden", "toggle-expand", open), "hidden");
  // Deliberate collapse: no guards, even during onboarding — it is
  // the only manual way to hide the island mid-onboarding.
  assert.equal(
    nextVisible(
      "expanded",
      "toggle-expand",
      withGuards({ onboardingOpen: true, pinned: true }),
    ),
    "compact",
  );
});

test("handle-click opens from hidden only", () => {
  assert.equal(nextVisible("hidden", "handle-click", open), "compact");
  assert.equal(nextVisible("compact", "handle-click", open), "compact");
  assert.equal(nextVisible("expanded", "handle-click", open), "expanded");
});

test("bell always expands", () => {
  for (const state of states) {
    assert.equal(nextVisible(state, "bell", open), "expanded");
  }
});

test("onboarding-open expands from anywhere", () => {
  for (const state of states) {
    assert.equal(nextVisible(state, "onboarding-open", open), "expanded");
  }
});

test("launch-start shows compact, never during onboarding", () => {
  assert.equal(nextVisible("hidden", "launch-start", open), "compact");
  assert.equal(
    nextVisible("hidden", "launch-start", withGuards({ onboardingOpen: true })),
    "hidden",
  );
});

test("launch-hint-timeout collapses only a compact, unhovered island", () => {
  assert.equal(
    nextVisible("compact", "launch-hint-timeout", open),
    "hidden",
  );
  assert.equal(
    nextVisible(
      "compact",
      "launch-hint-timeout",
      withGuards({ hovering: true }),
    ),
    "compact",
  );
  assert.equal(
    nextVisible(
      "compact",
      "launch-hint-timeout",
      withGuards({ onboardingOpen: true }),
    ),
    "compact",
  );
  assert.equal(
    nextVisible("expanded", "launch-hint-timeout", open),
    "expanded",
  );
});

test("shouldArmAutoClose blocks while onboarding, pinned, hovering, or hinting", () => {
  assert.equal(shouldArmAutoClose(open), true);
  assert.equal(
    shouldArmAutoClose(withGuards({ onboardingOpen: true })),
    false,
  );
  assert.equal(shouldArmAutoClose(withGuards({ pinned: true })), false);
  assert.equal(shouldArmAutoClose(withGuards({ hovering: true })), false);
  assert.equal(
    shouldArmAutoClose(withGuards({ launchHintActive: true })),
    false,
  );
});

test("pinned island ignores esc and shell-click but closes on explicit hotkey", () => {
  const pinned = withGuards({ pinned: true });
  assert.equal(nextVisible("expanded", "esc", pinned), "expanded");
  assert.equal(nextVisible("compact", "shell-click", pinned), "compact");
  assert.equal(nextVisible("expanded", "hotkey", pinned), "hidden");
});

test("toggle-expand from hidden stays hidden while bell and onboarding-open expand", () => {
  assert.equal(nextVisible("hidden", "toggle-expand", open), "hidden");
  assert.equal(nextVisible("hidden", "bell", open), "expanded");
  assert.equal(nextVisible("hidden", "onboarding-open", open), "expanded");
});
