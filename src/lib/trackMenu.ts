import { LogicalPosition } from "@tauri-apps/api/dpi";
import { Menu, type MenuItemOptions, type SubmenuOptions } from "@tauri-apps/api/menu";
import { api, type Track } from "@/lib/api";

/**
 * Native macOS menus for a track: the list's right-click menu and the "Open in…" button.
 * The items only call Rust commands (the file system stays on the Rust side).
 */

type OnError = (message: string) => void;

/** One item per app that can open the track, the default one first and marked. */
async function openWithItems(
  sessionId: number,
  track: Track,
  onError: OnError,
): Promise<MenuItemOptions[]> {
  const apps = await api.trackApps(sessionId, track.id);
  if (apps.length === 0) return [{ text: "No apps for this file", enabled: false }];
  return apps.map((app) => ({
    text: app.isDefault ? `${app.name} (default)` : app.name,
    action: () => void api.openTrack(sessionId, track.id, app.path).catch((e) => onError(String(e))),
  }));
}

/** Right-click menu of a track row. */
export async function showTrackMenu(
  sessionId: number,
  track: Track,
  { onPlay, onError }: { onPlay: () => void; onError: OnError },
): Promise<void> {
  const gone = track.status === "missing";
  let openWith: SubmenuOptions;
  try {
    openWith = gone
      ? { text: "Open With", enabled: false, items: [] }
      : { text: "Open With", items: await openWithItems(sessionId, track, onError) };
  } catch (e) {
    onError(String(e));
    return;
  }
  const menu = await Menu.new({
    items: [
      { text: "Play", enabled: !gone, action: onPlay },
      { item: "Separator" },
      {
        text: "Reveal in Finder",
        accelerator: "CmdOrCtrl+R",
        enabled: !gone,
        action: () =>
          void api.revealTrack(sessionId, track.id).catch((e) => onError(String(e))),
      },
      openWith,
    ],
  });
  await menu.popup();
}

/** The "Open in…" menu, dropped below `anchor`. */
export async function showOpenWithMenu(
  sessionId: number,
  track: Track,
  anchor: HTMLElement,
  onError: OnError,
): Promise<void> {
  try {
    const menu = await Menu.new({ items: await openWithItems(sessionId, track, onError) });
    const r = anchor.getBoundingClientRect();
    await menu.popup(new LogicalPosition(r.left, r.bottom + 4));
  } catch (e) {
    onError(String(e));
  }
}
