import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const source = join(import.meta.dir, "..", "..");
const entrySnapshot = readFileSync(join(source, "bin/agent-guard"));
const entrySnapshotSha256 = createHash("sha256").update(entrySnapshot).digest("hex");

export type Fault = "slow-reason" | "large-reason";

export interface FaultResult {
  fault: Fault;
  exit: number;
  signalCode: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
  milliseconds: number;
  descendantsAliveAtReturn: number[];
  producerBytesWritten: number | null;
  producerControl: { bytesWritten: number; outputLength: number; outputMatchesPayload: boolean } | null;
  entrySnapshotSha256: string;
  runnerEvidence: { entryPid: number | null; runnerPid: number; processGroupId: number };
}

export async function runFault(fault: Fault): Promise<FaultResult> {
  const home = mkdtempSync(join(tmpdir(), "agent-guard-fault-home-"));
  const pkg = join(home, "package");
  const bin = join(home, "bin");
  mkdirSync(join(pkg, "bin"), { recursive: true });
  mkdirSync(join(pkg, "src"), { recursive: true });
  mkdirSync(bin);
  const entryPath = join(pkg, "bin/agent-guard");
  const entrySource = entrySnapshot.toString("utf8");
  if (!entrySource.includes("pid=$!\n")) throw new Error("Entry snapshot no longer exposes the runner PID assignment");
  writeFileSync(entryPath, entrySource.replace("pid=$!\n", 'pid=$!\nprintf \'%s\\n\' "$pid" > "$HOME/entry.pid"\n'));
  chmodSync(join(pkg, "bin/agent-guard"), 0o755);
  writeFileSync(join(pkg, "src/guard.ts"), "// synthetic entry prerequisite\n");
  writeFileSync(join(pkg, "src/runner.ts"), "// synthetic entry prerequisite\n");
  writeFileSync(join(pkg, "tsconfig.json"), "{}\n");

  const reason = "DENIED: synthetic canary reason";
  const producer = `
import { writeSync } from "node:fs";
import { writeFileSync as writeProgress } from "node:fs";
export function writePayload(progressPath) {
const payload = Buffer.from("x".repeat(131_072) + "\\n");
let offset = 0;
while (offset < payload.length) {
  const written = writeSync(1, payload, offset, payload.length - offset);
  if (written <= 0) throw new Error("Synthetic producer made no write progress");
  offset += written;
  writeProgress(progressPath, String(offset));
}
return offset;
}
`;
  const launcher = `
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dlopen } from "bun:ffi";
const home = process.env.HOME!;
const libc = dlopen("/usr/lib/libSystem.B.dylib", {
  getpgid: { args: ["i32"], returns: "i32" },
});
const processGroupId = libc.symbols.getpgid(0);
libc.close();
writeFileSync(join(home, "runner.meta"), JSON.stringify({ runnerPid: process.pid, processGroupId }));
const workerPath = join(home, "worker.js");
const worker = Bun.spawn([process.execPath, workerPath], {
  stdin: "pipe",
  stdout: "pipe",
  stderr: "pipe",
  env: process.env,
});
const forward = async (stream) => { for await (const chunk of stream) process.stdout.write(chunk); };
const output = Promise.all([forward(worker.stdout), forward(worker.stderr)]);
writeFileSync(join(home, "runner.pid"), String(process.pid));
writeFileSync(join(home, "worker.pid"), String(worker.pid));
await Bun.file(join(home, "worker.ready")).exists().then(async (ready) => {
  while (!ready) {
    await Bun.sleep(10);
    ready = await Bun.file(join(home, "worker.ready")).exists();
  }
});
worker.stdin.write("go\\n");
worker.stdin.end();
const exit = await worker.exited;
await output;
process.exitCode = exit;
`;
  const worker = `
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { writePayload } from "./producer.js";
const home = process.env.HOME;
const fault = process.env.AGENT_GUARD_TEST_FAULT;
writeFileSync(join(home, "worker.ready"), "ready");
for await (const _ of Bun.stdin.stream()) break;
if (fault === "slow-reason") await Bun.sleep(3_200);
if (fault === "large-reason") {
  writePayload(join(home, "producer-bytes.txt"));
}
else process.stdout.write("${reason}\\n");
process.exitCode = fault === "slow-reason" ? 0 : 2;
`;
  const shim = `#!/bin/sh\nexec '${process.execPath}' "$HOME/launcher.ts"\n`;
  writeFileSync(join(bin, "bun"), shim);
  chmodSync(join(bin, "bun"), 0o755);
  writeFileSync(join(home, "launcher.ts"), launcher);
  writeFileSync(join(home, "worker.js"), worker);
  writeFileSync(join(home, "producer.js"), producer);
  writeFileSync(
    join(home, "producer-control.js"),
    `import { writePayload } from "./producer.js"; import { join } from "node:path"; const bytes = writePayload(join(process.env.HOME, "producer-control-bytes.txt"));`,
  );

  let entry: Bun.Subprocess<Buffer, "pipe", "pipe"> | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const trackedPids = new Set<number>();
  const processGroups = new Set<number>();
  const cleanup = () => {
    const groupFailures: unknown[] = [];
    const processFailures: unknown[] = [];
    if (entry) processGroups.add(entry.pid);
    for (const name of ["runner.pid", "worker.pid"]) {
      const path = join(home, name);
      if (!existsSync(path)) continue;
      const pid = Number(readFileSync(path, "utf8"));
      if (!Number.isInteger(pid) || pid <= 0) continue;
      trackedPids.add(pid);
      if (name === "runner.pid") processGroups.add(pid);
    }
    for (const pgid of processGroups) {
      try {
        process.kill(-pgid, "SIGKILL");
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ESRCH") groupFailures.push(error);
      }
    }
    for (const pid of trackedPids) {
      try {
        process.kill(pid, "SIGKILL");
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ESRCH") processFailures.push(error);
      }
    }
    const survivors = [...trackedPids].filter((pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
        processFailures.push(error);
        return true;
      }
    });
    if (survivors.length > 0 || processFailures.length > 0) {
      throw new AggregateError([...groupFailures, ...processFailures], `Could not clean up synthetic fault injection processes${survivors.length ? `: ${survivors.join(",")}` : ""}`);
    }
  };
  const started = performance.now();
  try {
    const spawned = Bun.spawn({
      cmd: [join(pkg, "bin/agent-guard"), "--runtime", "codex"],
      cwd: home,
      env: { HOME: home, PATH: `${bin}:/usr/bin:/bin`, AGENT_GUARD_TEST_FAULT: fault },
      stdin: Buffer.from('{"tool_input":{"command":"synthetic-canary"}}'),
      stdout: "pipe",
      stderr: "pipe",
      detached: true,
    });
    entry = spawned;
    const stdoutRead = new Response(spawned.stdout).text();
    const stderrRead = new Response(spawned.stderr).text();
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Fault ${fault} exceeded 8s`)), 8_000);
    });
    const exit = await Promise.race([spawned.exited, timeout]);
    const signalCode = spawned.signalCode;
    const milliseconds = performance.now() - started;
    const tracked = ["runner.pid", "worker.pid"]
      .filter((name) => existsSync(join(home, name)))
      .map((name) => Number(readFileSync(join(home, name), "utf8")))
      .filter((pid) => Number.isInteger(pid) && pid > 0);
    const descendantsAliveAtReturn = tracked.filter((pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
        throw error;
      }
    });

    // Terminate only after the at-return snapshot; these signals are harness cleanup.
    cleanup();
    const [stdout, stderr] = await Promise.all([stdoutRead, stderrRead]);
    const entryPidPath = join(home, "entry.pid");
    const entryPid = existsSync(entryPidPath) ? Number(readFileSync(entryPidPath, "utf8")) : null;
    const runnerEvidence = {
      entryPid,
      ...(JSON.parse(readFileSync(join(home, "runner.meta"), "utf8")) as { runnerPid: number; processGroupId: number }),
    };
    const producerBytesPath = join(home, "producer-bytes.txt");
    const producerBytesWritten = existsSync(producerBytesPath) ? Number(readFileSync(producerBytesPath, "utf8")) : null;
    let producerControl: FaultResult["producerControl"] = null;
    if (fault === "large-reason") {
      const control = Bun.spawn([process.execPath, join(home, "producer-control.js")], {
        env: { HOME: home, PATH: `${bin}:/usr/bin:/bin` },
        stdout: "pipe",
        stderr: "pipe",
      });
      const controlOutput = await new Response(control.stdout).arrayBuffer();
      const [controlExit, controlStderr] = await Promise.all([control.exited, new Response(control.stderr).text()]);
      if (controlExit !== 0) throw new Error(`Producer control failed (${controlExit}): ${controlStderr}`);
      const controlBytes = Number(readFileSync(join(home, "producer-control-bytes.txt"), "utf8"));
      const expected = Buffer.from("x".repeat(131_072) + "\n");
      producerControl = {
        bytesWritten: controlBytes,
        outputLength: controlOutput.byteLength,
        outputMatchesPayload: Buffer.from(controlOutput).equals(expected),
      };
    }
    return { fault, exit, signalCode, stdout, stderr, milliseconds, descendantsAliveAtReturn, producerBytesWritten, producerControl, entrySnapshotSha256, runnerEvidence };
  } finally {
    if (timer) clearTimeout(timer);
    cleanup();
    if (entry) {
      await entry.exited;
    }
    rmSync(home, { recursive: true, force: true });
  }
}
