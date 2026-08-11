import type { TerminalWindowNode } from "@/lib/tabWindows";
import { collectSessionPanes } from "@/lib/workspaceTabs";
import type {
  ActivityBarLayout,
  SessionPane,
  Tab,
  UiConfig,
  WorkspaceSessionType,
} from "@/types/global";

export const NON_PANEL_IDS = new Set(["settings", "lock", "quickCmdBar", "serialSend"]);

/** Panels that never join the multi-open stack and are always shown on their own. */
export const EXCLUSIVE_PANEL_IDS = new Set(["aiAssistant"]);

const MONITOR_PANEL_VISIBILITY: Record<string, (ui: UiConfig) => boolean> = {
  notes: (ui) => ui.show_notes_panel ?? true,
  resourceMonitor: (ui) => ui.show_remote_stats ?? true,
  gpuMonitor: (ui) => ui.show_gpu_monitor ?? false,
  ascendNpuMonitor: (ui) => ui.show_ascend_npu_monitor ?? false,
  processManager: (ui) => ui.show_process_manager ?? false,
  dockerManager: (ui) => ui.show_docker_manager ?? false,
};

export type TrayAction =
  | { type: "open_new_session"; targetWindowLabel?: string | null }
  | { type: "focus_session"; sessionId: string; targetWindowLabel?: string | null }
  | {
      type: "open_panel";
      panelId: "activeSessions" | "syncBackupHistory";
      targetWindowLabel?: string | null;
    }
  | { type: "open_settings"; targetWindowLabel?: string | null }
  | { type: "lock_screen"; targetWindowLabel?: string | null }
  | { type: "check_updates"; targetWindowLabel?: string | null }
  | { type: "request_quit"; targetWindowLabel?: string | null };

export function canCreateSessionFromPane(
  pane: Pick<SessionPane, "type" | "connectionId"> | null | undefined,
): pane is Pick<SessionPane, "type" | "connectionId"> {
  return !!pane && (pane.type === "Local" || !!pane.connectionId);
}

export function hasLiveSession<T extends Pick<SessionPane, "connecting" | "connectError">>(
  pane: T | null | undefined,
): pane is T {
  return !!pane && !pane.connecting && !pane.connectError;
}

export function isNonSerialSessionType(type: WorkspaceSessionType): boolean {
  return type === "SSH" || type === "Local" || type === "Telnet";
}

export function getItemSide(id: string, layout: ActivityBarLayout): "left" | "right" | null {
  if (layout.left_top.includes(id) || layout.left_bottom.includes(id)) return "left";
  if (layout.right_top.includes(id) || layout.right_bottom.includes(id)) return "right";
  return null;
}

export function isActivityItemVisible(id: string, ui: UiConfig): boolean {
  return MONITOR_PANEL_VISIBILITY[id]?.(ui) ?? true;
}

export function getVisibleActivityIds(ids: string[], ui: UiConfig): string[] {
  return ids.filter((id) => isActivityItemVisible(id, ui));
}

function getSidePanelOrder(layout: ActivityBarLayout, side: "left" | "right"): string[] {
  return side === "left"
    ? [...layout.left_top, ...layout.left_bottom]
    : [...layout.right_top, ...layout.right_bottom];
}

/** Panels currently visible on one side, ordered by activity bar icon order. */
export function getSideOpenPanels(
  ui: UiConfig,
  side: "left" | "right",
  multiOpen: boolean,
): string[] {
  const active = side === "left" ? ui.active_left_panel : ui.active_right_panel;
  if (!multiOpen) {
    return active && isActivityItemVisible(active, ui) ? [active] : [];
  }
  const open = new Set((side === "left" ? ui.left_open_panels : ui.right_open_panels) ?? []);
  if (open.size === 0) return [];
  return getSidePanelOrder(ui.activity_bar_layout, side).filter(
    (id) =>
      open.has(id) &&
      isActivityItemVisible(id, ui) &&
      !NON_PANEL_IDS.has(id) &&
      !EXCLUSIVE_PANEL_IDS.has(id),
  );
}

/** Exclusive panel currently shown on its own over the stack (multi-open mode only). */
export function getSideOverlayPanel(
  ui: UiConfig,
  side: "left" | "right",
  multiOpen: boolean,
): string | null {
  if (!multiOpen) return null;
  const active = side === "left" ? ui.active_left_panel : ui.active_right_panel;
  return active && isActivityItemVisible(active, ui) && EXCLUSIVE_PANEL_IDS.has(active)
    ? active
    : null;
}

/** Toggle a panel in the multi-open stack of its side (activity bar click). */
export function buildMultiPanelToggleUpdate(
  prev: UiConfig,
  panelId: string,
  side: "left" | "right",
): Partial<UiConfig> {
  const openList = (side === "left" ? prev.left_open_panels : prev.right_open_panels) ?? [];
  const active = side === "left" ? prev.active_left_panel : prev.active_right_panel;

  if (EXCLUSIVE_PANEL_IDS.has(panelId)) {
    const nextActive =
      active === panelId ? (openList.find((id) => !EXCLUSIVE_PANEL_IDS.has(id)) ?? null) : panelId;
    return side === "left" ? { active_left_panel: nextActive } : { active_right_panel: nextActive };
  }

  const isOpen = openList.includes(panelId);

  // Clicking an already-open stacked panel while an exclusive panel is shown
  // dismisses the exclusive panel and reveals the stack instead of closing it.
  if (isOpen && active && EXCLUSIVE_PANEL_IDS.has(active)) {
    return side === "left" ? { active_left_panel: panelId } : { active_right_panel: panelId };
  }

  const nextOpen = isOpen ? openList.filter((id) => id !== panelId) : [...openList, panelId];
  const nextActive =
    nextOpen.length === 0
      ? null
      : isOpen
        ? active && nextOpen.includes(active)
          ? active
          : nextOpen[0]
        : panelId;
  return side === "left"
    ? { left_open_panels: nextOpen, active_left_panel: nextActive }
    : { right_open_panels: nextOpen, active_right_panel: nextActive };
}

/** Ensure a panel is visible, respecting single/multi-open mode. */
export function buildPanelOpenUpdate(
  prev: UiConfig,
  panelId: string,
  multiOpen: boolean,
  fallbackSide: "left" | "right" = "left",
): Partial<UiConfig> {
  const side = getItemSide(panelId, prev.activity_bar_layout) ?? fallbackSide;
  if (!multiOpen) {
    return side === "right"
      ? {
          active_right_panel: panelId,
          ...(prev.active_left_panel === panelId ? { active_left_panel: null } : {}),
        }
      : {
          active_left_panel: panelId,
          ...(prev.active_right_panel === panelId ? { active_right_panel: null } : {}),
        };
  }
  if (EXCLUSIVE_PANEL_IDS.has(panelId)) {
    return side === "left" ? { active_left_panel: panelId } : { active_right_panel: panelId };
  }
  const openList = (side === "left" ? prev.left_open_panels : prev.right_open_panels) ?? [];
  const nextOpen = openList.includes(panelId) ? openList : [...openList, panelId];
  return side === "left"
    ? { left_open_panels: nextOpen, active_left_panel: panelId }
    : { right_open_panels: nextOpen, active_right_panel: panelId };
}

export function collectActiveNonSerialSessionIds(
  layout: TerminalWindowNode | null,
  tabsById: Map<string, Tab>,
) {
  if (!layout) return [];

  const sessionIds = new Set<string>();

  const visit = (node: TerminalWindowNode) => {
    if (node.kind === "split") {
      visit(node.first);
      visit(node.second);
      return;
    }

    for (const tabId of node.tabIds) {
      const tab = tabsById.get(tabId);
      if (!tab) continue;

      for (const pane of collectSessionPanes(tab.root)) {
        if (hasLiveSession(pane) && isNonSerialSessionType(pane.type)) {
          sessionIds.add(pane.sessionId);
        }
      }
    }
  };

  visit(layout);
  return [...sessionIds];
}
