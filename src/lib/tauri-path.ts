import { isTauriRuntime } from "@/lib/tauri";

export async function homeDir(): Promise<string> {
  if (isTauriRuntime()) {
    const mod = await import("@tauri-apps/api/path");
    return mod.homeDir();
  }
  throw new Error("homeDir is only available in the desktop runtime");
}

export async function join(...paths: string[]): Promise<string> {
  if (isTauriRuntime()) {
    const mod = await import("@tauri-apps/api/path");
    return mod.join(...paths);
  }
  return paths.join("/").replace(/\/+/g, "/");
}
