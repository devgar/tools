export async function copyToClipboard(text: string): Promise<void> {
  // Try wl-copy (Wayland) first, then xsel and xclip (X11).
  const tools: [string, string[]][] = [
    ["wl-copy", []],
    ["xsel",    ["--clipboard", "--input"]],
    ["xclip",   ["-selection", "clipboard"]],
  ];
  for (const [cmd, args] of tools) {
    try {
      const proc = Bun.spawn([cmd, ...args], {
        stdin: new Blob([text]),
        stdout: "pipe",
        stderr: "pipe",
      });
      await proc.exited;
      if (proc.exitCode === 0) return;
    } catch {
      // tool not found — try next
    }
  }
}
