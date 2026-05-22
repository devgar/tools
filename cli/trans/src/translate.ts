export async function runTranslate(text: string, src: string, tgt: string): Promise<string> {
  const cmd = `gawk -f <(curl -Ls --compressed https://git.io/translate) -- -s ${src} -t ${tgt} -b 2>/dev/null`;
  try {
    const proc = Bun.spawn(["bash", "-c", cmd], {
      stdin: new Blob([text]),
      stdout: "pipe",
      stderr: "pipe",
    });
    await proc.exited;
    const output = await Bun.readableStreamToText(proc.stdout);
    const firstLine = output.split("\n").find((l) => l.trim().length > 0) ?? "";
    return firstLine.trim();
  } catch {
    return "";
  }
}
