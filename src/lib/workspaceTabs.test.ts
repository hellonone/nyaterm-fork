import { describe, expect, it } from "vitest";
import type { RestorableTab } from "@/types/global";
import {
  createSessionPane,
  createWorkspaceTab,
  restoreTabFromPersistence,
  serializeTabsForPersistence,
} from "./workspaceTabs";

describe("workspaceTabs RDP persistence", () => {
  it("keeps legacy terminal tabs restorable without pane_kind", () => {
    const restored = restoreTabFromPersistence(
      {
        title: "Legacy SSH",
        session_type: "SSH",
        connection_id: "ssh-1",
      } as RestorableTab,
      0,
    );

    expect(restored).not.toBeNull();
    const leaf = restored?.root.kind === "leaf" ? restored.root : null;
    expect(leaf?.paneKind).toBe("terminal");
    expect(leaf?.type).toBe("SSH");
    expect(leaf?.connectionId).toBe("ssh-1");
  });

  it("serializes and restores RDP panes as graphical leaves", () => {
    const pane = createSessionPane("Windows", "RDP", "rdp-1", {
      id: "pane-rdp",
      sessionId: "session-rdp",
    });
    const [serialized] = serializeTabsForPersistence([createWorkspaceTab(pane, 0)]);

    expect(serialized.root?.kind).toBe("leaf");
    if (serialized.root?.kind === "leaf") {
      expect(serialized.root.pane_kind).toBe("rdp");
      expect(serialized.root.session_type).toBe("RDP");
    }

    const restored = restoreTabFromPersistence(serialized, 0);
    const leaf = restored?.root.kind === "leaf" ? restored.root : null;
    if (leaf?.paneKind !== "rdp") throw new Error("expected RDP leaf");
    expect(leaf?.type).toBe("RDP");
    expect(leaf?.display).toMatchObject({
      remoteWidth: 1920,
      remoteHeight: 1080,
      scaleMode: "fit",
    });
  });

  it("rejects mismatched RDP pane kind and terminal session type", () => {
    const restored = restoreTabFromPersistence(
      {
        title: "Invalid",
        session_type: "SSH",
        root: {
          kind: "leaf",
          pane_kind: "rdp",
          title: "Invalid",
          session_type: "SSH",
        },
      } as RestorableTab,
      0,
    );

    expect(restored).toBeNull();
  });
});
