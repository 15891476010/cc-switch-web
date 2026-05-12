import { isTauriRuntime } from "@/lib/tauri";

export async function getVersion(): Promise<string> {
  if (isTauriRuntime()) {
    const mod = await import("@tauri-apps/api/app");
    return mod.getVersion();
  }
  return import.meta.env.VITE_APP_VERSION || "web";
}
