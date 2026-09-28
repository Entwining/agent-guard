import { expect } from "bun:test";
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// The installed test copies report PIDs that the parent cannot recover after exit.
export function instrumentEntry(path: string): void {
  const source = readFileSync(path, "utf8");
  const marker = "pid=$!\n";
  if (!source.includes(marker)) throw new Error("Test entry no longer reports its coprocess PID");
  writeFileSync(path, source.replace(marker, 'pid=$!\nprint -r -- "P:$pid" >> "$AGENT_GUARD_TEST_PIDS"\n'));
}

export function instrumentRunner(path: string): void {
  const source = readFileSync(path, "utf8");
  const marker = "process.stdout.write(`AGENT_GUARD_PGID=${child.pid}\\n`);";
  if (!source.includes(marker)) throw new Error("Test runner no longer reports the guard process group");
  const log = "appendFileSync(process.env.AGENT_GUARD_TEST_PIDS!, `G:${child.pid}\\n`);";
  const prefix = 'import { appendFileSync } from "node:fs";\nappendFileSync(process.env.AGENT_GUARD_TEST_PIDS!, `P:${process.pid}\\n`);\n';
  writeFileSync(path, prefix + source.replace(marker, `${log}\n${marker}`));
}

export function exists(pid: number, group: boolean): boolean {
  try {
    process.kill(group ? -pid : pid, 0);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
    throw error;
  }
}

export function stop(pid: number, group: boolean): void {
  try {
    process.kill(group ? -pid : pid, "SIGKILL");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
  }
}

export async function run(h: string, cmd: string[], event: unknown, cwd = h, afterSpawn?: (pid: number) => void, beforeCleanup?: () => void | Promise<void>) {
  const started = performance.now();
  const log = join(h, `pids-${randomUUID()}`);
  const groups = new Set<number>();
  const pids = new Set<number>();
  let child: { pid: number; exited: Promise<number> } | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let failed = false;
  try {
    const spawned = Bun.spawn({
      cmd,
      cwd,
      stdin: Buffer.from(JSON.stringify(event)),
      stdout: "pipe",
      stderr: "pipe",
      detached: true,
      env: { HOME: h, PATH: `${join(h, "bin")}:/usr/bin:/bin`, AGENT_GUARD_TEST_PIDS: log },
    });
    child = spawned;
    groups.add(spawned.pid);
    afterSpawn?.(spawned.pid);
    const result = Promise.all([spawned.exited, new Response(spawned.stdout).text(), new Response(spawned.stderr).text()]);
    const timeout = new Promise<never>((_, reject) => {
      deadline = setTimeout(() => reject(new Error("Test hook did not finish within 7 seconds")), 7_000);
    });
    const [exit, stdout, stderr] = await Promise.race([result, timeout]);
    await beforeCleanup?.();
    return { exit, stdout, stderr, seconds: (performance.now() - started) / 1000 };
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    if (deadline) clearTimeout(deadline);
    if (existsSync(log)) {
      for (const line of readFileSync(log, "utf8").trim().split("\n")) {
        const [kind, number] = line.split(":");
        const pid = Number(number);
        if (!Number.isInteger(pid) || pid <= 0) continue;
        if (kind === "G") groups.add(pid);
        if (kind === "P") pids.add(pid);
      }
    }
    for (const pid of groups) stop(pid, true);
    for (const pid of pids) stop(pid, false);
    if (child) await child.exited;
    try {
      if (!failed) {
        for (let attempt = 0; attempt < 20 && ([...groups].some((pid) => exists(pid, true)) || [...pids].some((pid) => exists(pid, false))); attempt++) {
          await Bun.sleep(25);
        }
        expect([...groups].filter((pid) => exists(pid, true))).toEqual([]);
        expect([...pids].filter((pid) => exists(pid, false))).toEqual([]);
      }
    } finally {
      rmSync(log, { force: true });
    }
  }
}
