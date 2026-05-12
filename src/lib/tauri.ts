export type UnlistenFn = () => void;

type InvokeArgs = Record<string, unknown> | undefined;

const hasTauriInternals = (): boolean =>
  typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);

export const isTauriRuntime = hasTauriInternals;

const apiBase = (): string => {
  const envBase = import.meta.env.VITE_CC_SWITCH_API_BASE as string | undefined;
  if (envBase && envBase.trim()) return envBase.replace(/\/$/, "");
  return "";
};

export async function invoke<T = unknown>(
  command: string,
  args?: InvokeArgs,
): Promise<T> {
  if (hasTauriInternals()) {
    const mod = await import("@tauri-apps/api/core");
    return mod.invoke<T>(command, args);
  }

  const response = await fetch(`${apiBase()}/api/rpc/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {}),
  });

  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;

  if (!response.ok) {
    const message =
      payload && typeof payload === "object" && "error" in payload
        ? String((payload as { error: unknown }).error)
        : response.statusText;
    throw new Error(message);
  }

  return payload as T;
}

export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (hasTauriInternals()) {
    const mod = await import("@tauri-apps/api/event");
    return mod.listen<T>(event, handler);
  }

  return () => {};
}

export async function message(
  text: string,
  options?: { title?: string; kind?: string },
): Promise<void> {
  if (hasTauriInternals()) {
    const mod = await import("@tauri-apps/plugin-dialog");
    await mod.message(text, options as any);
    return;
  }

  window.alert(options?.title ? `${options.title}\n\n${text}` : text);
}

export async function exit(code = 0): Promise<void> {
  if (hasTauriInternals()) {
    const mod = await import("@tauri-apps/plugin-process");
    await mod.exit(code);
    return;
  }

  console.warn(`Exit requested with code ${code}; ignored in web runtime.`);
}

export async function relaunch(): Promise<void> {
  if (hasTauriInternals()) {
    const mod = await import("@tauri-apps/plugin-process");
    await mod.relaunch();
    return;
  }

  window.location.reload();
}
