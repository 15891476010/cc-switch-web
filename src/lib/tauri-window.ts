import { isTauriRuntime } from "@/lib/tauri";

type WindowLike = {
  isMaximized: () => Promise<boolean>;
  onResized: (handler: () => void) => Promise<() => void>;
  setDecorations: (decorated: boolean) => Promise<void>;
  minimize: () => Promise<void>;
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
};

const loadDesktopWindow = async (): Promise<WindowLike> => {
  const mod = await import("@tauri-apps/api/window");
  return mod.getCurrentWindow() as unknown as WindowLike;
};

const webWindow: WindowLike = {
  isMaximized: async () => false,
  onResized: async () => () => {},
  setDecorations: async () => {},
  minimize: async () => {},
  toggleMaximize: async () => {},
  close: async () => window.close(),
};

const desktopWindow: WindowLike = {
  isMaximized: async () => (await loadDesktopWindow()).isMaximized(),
  onResized: async (handler) => (await loadDesktopWindow()).onResized(handler),
  setDecorations: async (decorated) =>
    (await loadDesktopWindow()).setDecorations(decorated),
  minimize: async () => (await loadDesktopWindow()).minimize(),
  toggleMaximize: async () => (await loadDesktopWindow()).toggleMaximize(),
  close: async () => (await loadDesktopWindow()).close(),
};

export function getCurrentWindow(): WindowLike {
  if (!isTauriRuntime()) return webWindow;
  return desktopWindow;
}
