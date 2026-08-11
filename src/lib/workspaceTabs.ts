import type {
  PaneNode,
  PaneSplitDirection,
  RestorablePaneNode,
  RestorableTab,
  SessionPane,
  SplitPane,
  Tab,
  WorkspaceSessionType,
} from "@/types/global";

let workspaceIdCounter = 0;

export function createWorkspaceId(prefix: string) {
  workspaceIdCounter += 1;
  return `${prefix}-${Date.now()}-${workspaceIdCounter}`;
}

export function isSplitPane(node: PaneNode): node is SplitPane {
  return node.kind === "split";
}

export function isSessionPane(node: PaneNode): node is SessionPane {
  return node.kind === "leaf";
}

export function isTerminalPane(
  node: PaneNode,
): node is Extract<SessionPane, { paneKind: "terminal" }> {
  return isSessionPane(node) && node.paneKind === "terminal";
}

export function isRdpPane(node: PaneNode): node is Extract<SessionPane, { paneKind: "rdp" }> {
  return isSessionPane(node) && node.paneKind === "rdp";
}

export function createSessionPane(
  name: string,
  type: WorkspaceSessionType,
  connectionId?: string,
  overrides?: Partial<SessionPane>,
): SessionPane {
  const paneKind = type === "RDP" ? "rdp" : "terminal";
  return {
    id: overrides?.id ?? createWorkspaceId("pane"),
    kind: "leaf",
    paneKind,
    sessionId: overrides?.sessionId ?? createWorkspaceId("session"),
    name,
    type,
    connectionId,
    display:
      type === "RDP"
        ? ((overrides && "display" in overrides ? overrides.display : undefined) ?? {
            remoteWidth: 1920,
            remoteHeight: 1080,
            scaleMode: "fit",
          })
        : undefined,
    connecting: overrides?.connecting,
    createRequestId: overrides?.createRequestId,
    connectError: overrides?.connectError,
  } as SessionPane;
}

export function createWorkspaceTab(
  pane: SessionPane,
  persistOrder: number,
  extra?: Partial<Pick<Tab, "customName" | "tabColor">>,
): Tab {
  return {
    id: createWorkspaceId("tab"),
    persistOrder,
    activePaneId: pane.id,
    root: pane,
    customName: extra?.customName,
    tabColor: extra?.tabColor,
  };
}

export function collectSessionPanes(node: PaneNode): SessionPane[] {
  if (isSessionPane(node)) return [node];
  return [...collectSessionPanes(node.first), ...collectSessionPanes(node.second)];
}

export function getFirstSessionPane(node: PaneNode): SessionPane | null {
  if (isSessionPane(node)) return node;
  return getFirstSessionPane(node.first) ?? getFirstSessionPane(node.second);
}

export function findPaneById(node: PaneNode, paneId: string): PaneNode | null {
  if (node.id === paneId) return node;
  if (isSessionPane(node)) return null;
  return findPaneById(node.first, paneId) ?? findPaneById(node.second, paneId);
}

export function findSessionPaneById(node: PaneNode, paneId: string): SessionPane | null {
  const pane = findPaneById(node, paneId);
  return pane && isSessionPane(pane) ? pane : null;
}

export function findSessionPaneBySessionId(node: PaneNode, sessionId: string): SessionPane | null {
  if (isSessionPane(node)) return node.sessionId === sessionId ? node : null;
  return (
    findSessionPaneBySessionId(node.first, sessionId) ??
    findSessionPaneBySessionId(node.second, sessionId)
  );
}

function updatePaneTree(
  node: PaneNode,
  paneId: string,
  updater: (current: PaneNode) => PaneNode,
): PaneNode {
  if (node.id === paneId) return updater(node);
  if (isSessionPane(node)) return node;

  const nextFirst = updatePaneTree(node.first, paneId, updater);
  const nextSecond = updatePaneTree(node.second, paneId, updater);
  if (nextFirst === node.first && nextSecond === node.second) return node;
  return { ...node, first: nextFirst, second: nextSecond };
}

export function updateSessionPane(
  root: PaneNode,
  paneId: string,
  updates: Partial<
    Pick<
      SessionPane,
      | "sessionId"
      | "name"
      | "type"
      | "connectionId"
      | "connecting"
      | "connectError"
      | "createRequestId"
    >
  >,
): PaneNode {
  return updatePaneTree(root, paneId, (current) =>
    isSessionPane(current) ? ({ ...current, ...updates } as SessionPane) : current,
  );
}

export function splitSessionPane(
  root: PaneNode,
  paneId: string,
  direction: PaneSplitDirection,
  newPane: SessionPane,
): PaneNode {
  return updatePaneTree(root, paneId, (current) => {
    if (!isSessionPane(current)) return current;
    return {
      id: createWorkspaceId("split"),
      kind: "split",
      direction,
      ratio: 0.5,
      first: current,
      second: newPane,
    };
  });
}

export function updateSplitRatio(root: PaneNode, splitId: string, ratio: number): PaneNode {
  return updatePaneTree(root, splitId, (current) => {
    if (!isSplitPane(current)) return current;
    return { ...current, ratio: clampSplitRatio(ratio) };
  });
}

export function removeSessionPane(root: PaneNode, paneId: string): PaneNode | null {
  if (isSessionPane(root)) return root.id === paneId ? null : root;

  const nextFirst = removeSessionPane(root.first, paneId);
  const nextSecond = removeSessionPane(root.second, paneId);

  if (!nextFirst && !nextSecond) return null;
  if (!nextFirst) return nextSecond;
  if (!nextSecond) return nextFirst;
  if (nextFirst === root.first && nextSecond === root.second) return root;
  return { ...root, first: nextFirst, second: nextSecond };
}

export function getActivePane(tab: Tab): SessionPane | null {
  return findSessionPaneById(tab.root, tab.activePaneId) ?? getFirstSessionPane(tab.root);
}

export function getTabDisplayName(tab: Tab): string {
  return tab.customName || getActivePane(tab)?.name || "Session";
}

export function getTabActiveSessionId(tab: Tab) {
  return getActivePane(tab)?.sessionId ?? null;
}

export function getTabActiveConnectionId(tab: Tab) {
  return getActivePane(tab)?.connectionId;
}

export function getTabActiveType(tab: Tab) {
  return getActivePane(tab)?.type ?? null;
}

export function ensureActivePane(tab: Tab): Tab {
  const activePane = getActivePane(tab);
  if (!activePane || activePane.id === tab.activePaneId) return tab;
  return { ...tab, activePaneId: activePane.id };
}

export function getNextPersistOrder(tabs: Tab[]) {
  return tabs.reduce((max, tab) => Math.max(max, tab.persistOrder), -1) + 1;
}

export function insertTabAfter(tabs: Tab[], afterTabId: string, newTab: Tab): Tab[] {
  const index = tabs.findIndex((tab) => tab.id === afterTabId);
  if (index === -1) return [...tabs, newTab];
  const next = [...tabs];
  next.splice(index + 1, 0, newTab);
  return next;
}

export function moveTab(tabs: Tab[], fromTabId: string, toIndex: number): Tab[] {
  const fromIndex = tabs.findIndex((tab) => tab.id === fromTabId);
  if (fromIndex === -1) return tabs;
  const boundedIndex = Math.max(0, Math.min(tabs.length - 1, toIndex));
  if (fromIndex === boundedIndex) return tabs;

  const next = [...tabs];
  const [tab] = next.splice(fromIndex, 1);
  next.splice(boundedIndex, 0, tab);
  return next;
}

export function findTabBySessionId(tabs: Tab[], sessionId: string) {
  return tabs.find((tab) => findSessionPaneBySessionId(tab.root, sessionId));
}

export function findPaneBySessionId(tab: Tab, sessionId: string) {
  return findSessionPaneBySessionId(tab.root, sessionId);
}

function serializePane(node: PaneNode): RestorablePaneNode {
  if (isSessionPane(node)) {
    return {
      id: node.id,
      kind: "leaf",
      pane_kind: node.paneKind,
      title: node.name,
      session_type: node.type,
      connection_id: node.connectionId,
    };
  }

  return {
    id: node.id,
    kind: "split",
    direction: node.direction,
    ratio: clampSplitRatio(node.ratio),
    first: serializePane(node.first),
    second: serializePane(node.second),
  };
}

export function serializeTabsForPersistence(tabs: Tab[]): RestorableTab[] {
  return [...tabs]
    .sort((a, b) => a.persistOrder - b.persistOrder)
    .map((tab) => ({
      title: getTabDisplayName(tab),
      session_type: getActivePane(tab)?.type ?? "Local",
      connection_id: getActivePane(tab)?.connectionId,
      custom_name: tab.customName,
      tab_color: tab.tabColor,
      locked: tab.locked,
      active_pane_id: tab.activePaneId,
      root: serializePane(tab.root),
    }));
}

function createLegacyPaneNode(tab: RestorableTab): RestorablePaneNode | null {
  const type = normalizeSessionType(tab.session_type);
  if (!type) return null;

  return {
    kind: "leaf",
    title: tab.title || "Session",
    session_type: type,
    connection_id: tab.connection_id,
  };
}

export function normalizeSessionType(value: string): WorkspaceSessionType | null {
  switch (value) {
    case "SSH":
      return "SSH";
    case "Local":
    case "local":
      return "Local";
    case "Telnet":
      return "Telnet";
    case "Serial":
      return "Serial";
    case "RDP":
    case "rdp":
      return "RDP";
    default:
      return null;
  }
}

function restorePane(node: RestorablePaneNode): PaneNode | null {
  if (node.kind === "leaf") {
    const type = normalizeSessionType(node.session_type);
    if (!type) return null;
    const paneKind = node.pane_kind ?? (type === "RDP" ? "rdp" : "terminal");
    if ((paneKind === "rdp") !== (type === "RDP")) return null;
    return {
      id: node.id || createWorkspaceId("pane"),
      kind: "leaf",
      paneKind,
      sessionId: createWorkspaceId("pending"),
      name: node.title,
      type,
      connectionId: node.connection_id,
      display:
        paneKind === "rdp"
          ? {
              remoteWidth: 1920,
              remoteHeight: 1080,
              scaleMode: "fit",
            }
          : undefined,
      connecting: true,
      createRequestId: crypto.randomUUID(),
    } as SessionPane;
  }

  const first = restorePane(node.first);
  const second = restorePane(node.second);
  if (!first && !second) return null;
  if (!first) return second;
  if (!second) return first;

  return {
    id: node.id || createWorkspaceId("split"),
    kind: "split",
    direction: node.direction,
    ratio: clampSplitRatio(node.ratio),
    first,
    second,
  };
}

export function restoreTabFromPersistence(tab: RestorableTab, persistOrder: number): Tab | null {
  const restorableRoot = tab.root ?? createLegacyPaneNode(tab);
  if (!restorableRoot) return null;

  const root = restorePane(restorableRoot);
  if (!root) return null;

  const restored: Tab = {
    id: createWorkspaceId("tab"),
    persistOrder,
    activePaneId: tab.active_pane_id || getFirstSessionPane(root)?.id || "",
    root,
    customName: tab.custom_name,
    tabColor: tab.tab_color,
    locked: tab.locked,
  };

  return ensureActivePane(restored);
}

export function clampSplitRatio(ratio: number) {
  return Math.max(0.2, Math.min(0.8, ratio));
}
