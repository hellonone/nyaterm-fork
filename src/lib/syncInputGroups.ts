import type { SyncGroup, Tab } from "@/types/global";
import { collectSessionPanes } from "./workspaceTabs";

let groupIdCounter = 0;

function createGroupId() {
  groupIdCounter += 1;
  return `sync-group-${Date.now()}-${groupIdCounter}`;
}

const GROUP_COLORS = [
  "#3b82f6",
  "#ef4444",
  "#22c55e",
  "#f59e0b",
  "#8b5cf6",
  "#ec4899",
  "#06b6d4",
  "#f97316",
];

export function nextGroupColor(existing: SyncGroup[]): string {
  const usedColors = new Set(existing.map((g) => g.color));
  return (
    GROUP_COLORS.find((c) => !usedColors.has(c)) ??
    GROUP_COLORS[existing.length % GROUP_COLORS.length]
  );
}

export function createSyncGroup(name: string, color: string): SyncGroup {
  return {
    id: createGroupId(),
    name,
    color,
    sessionIds: [],
    pausedSessionIds: [],
    enabled: true,
  };
}

export function addSessionToGroup(group: SyncGroup, sessionId: string): SyncGroup {
  if (group.sessionIds.includes(sessionId)) return group;
  return { ...group, sessionIds: [...group.sessionIds, sessionId] };
}

export function removeSessionFromGroup(group: SyncGroup, sessionId: string): SyncGroup {
  if (!group.sessionIds.includes(sessionId)) return group;
  return {
    ...group,
    sessionIds: group.sessionIds.filter((id) => id !== sessionId),
    pausedSessionIds: group.pausedSessionIds.filter((id) => id !== sessionId),
  };
}

export function toggleGroupEnabled(group: SyncGroup): SyncGroup {
  return { ...group, enabled: !group.enabled };
}

export function pauseSessionInGroup(group: SyncGroup, sessionId: string): SyncGroup {
  if (!group.sessionIds.includes(sessionId) || group.pausedSessionIds.includes(sessionId))
    return group;
  return { ...group, pausedSessionIds: [...group.pausedSessionIds, sessionId] };
}

export function resumeSessionInGroup(group: SyncGroup, sessionId: string): SyncGroup {
  if (!group.pausedSessionIds.includes(sessionId)) return group;
  return { ...group, pausedSessionIds: group.pausedSessionIds.filter((id) => id !== sessionId) };
}

export function isSessionPausedInGroup(group: SyncGroup, sessionId: string): boolean {
  return group.pausedSessionIds.includes(sessionId);
}

/**
 * Given a session id, return all distinct peer session ids that should receive
 * the same input based on active sync groups. Paused sessions are excluded.
 */
export function getSyncPeers(sessionId: string, groups: SyncGroup[]): string[] {
  const peers = new Set<string>();
  for (const g of groups) {
    if (!g.enabled || !g.sessionIds.includes(sessionId)) continue;
    if (g.pausedSessionIds.includes(sessionId)) continue;
    for (const sid of g.sessionIds) {
      if (sid !== sessionId && !g.pausedSessionIds.includes(sid)) peers.add(sid);
    }
  }
  return [...peers];
}

export function getSessionInputPeerIds(
  sessionId: string,
  groups: SyncGroup[],
  tabs: Tab[],
  broadcastToAll: boolean,
): string[] {
  const peers = new Set(getSyncPeers(sessionId, groups));

  if (broadcastToAll) {
    for (const tab of tabs) {
      for (const pane of collectSessionPanes(tab.root)) {
        if (
          pane.paneKind === "terminal" &&
          pane.sessionId !== sessionId &&
          !pane.connecting &&
          !pane.connectError
        ) {
          peers.add(pane.sessionId);
        }
      }
    }
  }

  return [...peers];
}

/** Check whether a session belongs to any enabled sync group (not paused). */
export function isSessionSynced(sessionId: string, groups: SyncGroup[]): boolean {
  return groups.some(
    (g) => g.enabled && g.sessionIds.includes(sessionId) && !g.pausedSessionIds.includes(sessionId),
  );
}

/** Return the first enabled group a session actively belongs to (not paused). */
export function getActiveGroupForSession(sessionId: string, groups: SyncGroup[]): SyncGroup | null {
  return groups.find((g) => g.enabled && g.sessionIds.includes(sessionId)) ?? null;
}

/** Return all groups a session belongs to. */
export function getGroupsForSession(sessionId: string, groups: SyncGroup[]): SyncGroup[] {
  return groups.filter((g) => g.sessionIds.includes(sessionId));
}

/** Cleanup: remove a session id from all groups (e.g. when the session closes). */
export function purgeSessionFromGroups(sessionId: string, groups: SyncGroup[]): SyncGroup[] {
  let changed = false;
  const next = groups.map((g) => {
    if (!g.sessionIds.includes(sessionId)) return g;
    changed = true;
    return removeSessionFromGroup(g, sessionId);
  });
  return changed ? next : groups;
}
