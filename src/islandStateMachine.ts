// Pure transition rules for the island visibility state machine.
//
// The component (App.tsx) is a thin shell around this module: every
// handler asks nextVisible() what the next state is, and showIsland()
// applies it. The guards live here exactly once, so the rules can be
// tested without React — see tests/islandStateMachine.test.ts
// (node:test, no dependency).
//
// Rules, as the component implements them:
// - The auto-close timer steps down unconditionally when it fires;
//   the guards are checked when the timer is ARMED
//   (shouldArmAutoClose), and arming is cancelled by pinning or
//   hovering (clearAutoCloseTimer). So "auto-close-timeout" itself
//   takes no guards.
// - Esc respects pin + onboarding; the hotkey respects onboarding
//   only (it is the explicit toggle and also closes a pinned
//   island); an empty-space click respects pin + onboarding, like
//   Esc — "Keep open" means the island stays open, so a stray
//   shell click must not collapse it. The hotkey and the
//   brand/clock toggle remain the deliberate ways to hide a pinned
//   island.
// - The brand/clock toggle is the deliberate collapse: it has no
//   guards, and it is the only manual way to hide the island while
//   the onboarding is open, so it must stay unguarded.
// - "hot-zone-leave" changes nothing by itself; it only arms the
//   timer (via shouldArmAutoClose).
// - "launch-start" is gated by the caller too (it only runs on a
//   non-onboarding launch), and guarded here as well.

export type IslandVisible = "hidden" | "compact" | "expanded";

export type IslandEvent =
  | "hot-zone-enter"
  | "hot-zone-leave"
  | "auto-close-timeout"
  | "esc"
  | "hotkey"
  | "shell-click"
  | "toggle-expand"
  | "handle-click"
  | "bell"
  | "onboarding-open"
  | "launch-start"
  | "launch-hint-timeout";

export interface IslandGuards {
  /** The first-run onboarding is open (expanded panel). */
  onboardingOpen: boolean;
  /** The island is pinned open ("Keep open"). */
  pinned: boolean;
  /** The pointer is over the island. */
  hovering: boolean;
  /** The ~6 s launch hint is still showing. */
  launchHintActive: boolean;
}

function stepDown(current: IslandVisible): IslandVisible {
  if (current === "expanded") {
    return "compact";
  }
  if (current === "compact") {
    return "hidden";
  }
  return current;
}

export function nextVisible(
  current: IslandVisible,
  event: IslandEvent,
  guards: IslandGuards,
): IslandVisible {
  switch (event) {
    case "hot-zone-enter":
      return current === "hidden" ? "compact" : current;
    case "hot-zone-leave":
      return current;
    case "auto-close-timeout":
      return stepDown(current);
    case "esc":
      if (guards.pinned || guards.onboardingOpen) {
        return current;
      }
      return stepDown(current);
    case "hotkey":
      if (guards.onboardingOpen) {
        return current;
      }
      return current === "hidden" ? "compact" : "hidden";
    case "shell-click":
      if (guards.pinned || guards.onboardingOpen) {
        return current;
      }
      return stepDown(current);
    case "toggle-expand":
      if (current === "compact") {
        return "expanded";
      }
      if (current === "expanded") {
        return "compact";
      }
      return current;
    case "handle-click":
      return current === "hidden" ? "compact" : current;
    case "bell":
      return "expanded";
    case "onboarding-open":
      return "expanded";
    case "launch-start":
      return guards.onboardingOpen ? current : "compact";
    case "launch-hint-timeout":
      if (guards.onboardingOpen || guards.hovering || current !== "compact") {
        return current;
      }
      return "hidden";
  }
}

/** Whether the auto-close timer may be armed right now. */
export function shouldArmAutoClose(guards: IslandGuards): boolean {
  return (
    !guards.onboardingOpen &&
    !guards.pinned &&
    !guards.hovering &&
    !guards.launchHintActive
  );
}
