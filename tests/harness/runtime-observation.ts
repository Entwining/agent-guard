import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
interface ProcessRow {
  pid: number;
  ppid: number;
  started: string;
  role?: string;
}

export async function observeGuard(runtime: string, input: string, cwd?: string) {
  if (process.env["AGENT_GUARD_TEST_ABLATE"] === "1")
    return { status: 0, signal: null, stdout: "", stderr: "", spawnError: undefined, timedOut: false, ms: 0, observed: [], survivors: [], observationError: undefined };
  const start = performance.now();
  const child = spawn(process.env["AGENT_GUARD_TEST_ENTRY"]!, ["--runtime", runtime], { cwd, env: process.env, stdio: ["pipe", "pipe", "pipe"] });
  let stderr = "";
  let stdout = "";
  child.stderr.on("data", (data) => {
    stderr += data.toString();
  });
  child.stdout.on("data", (data) => {
    stdout += data.toString();
  });
  child.stdin.end(input);
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    child.kill("SIGKILL");
  }, 4500);
  let spawnError: string | undefined;
  const { status, signal } = await new Promise<{ status: number | null; signal: NodeJS.Signals | null }>((resolve) => {
    child.on("error", (error) => {
      spawnError = error.message;
    });
    child.on("close", (code, signal) => resolve({ status: code, signal }));
  });
  const ms = performance.now() - start;
  clearTimeout(timer);
  let observationError: string | undefined;
  let observed: ProcessRow[] = child.pid ? [{ pid: child.pid, ppid: process.pid, started: String(Date.now()), role: "entry" }] : [];
  try {
    const path = process.env["AGENT_GUARD_TEST_PID_TRACE"];
    if (!path) throw new Error("AGENT_GUARD_TEST_PID_TRACE absent: runner/guard instrumentation not configured");
    observed = observed.concat(
      readFileSync(path, "utf8")
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as ProcessRow),
    );
    if (["runner", "guard"].some((role) => observed.filter((row) => row.role === role).length !== 1)) throw new Error("PID trace is incomplete: expected runner and guard roles");
  } catch (error) {
    observationError = String(error);
  }
  const survivors = observed.filter((row) => {
    try {
      process.kill(row.pid, 0);
      return true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ESRCH") observationError = String(error);
      return false;
    }
  });
  for (const row of survivors) {
    try {
      process.kill(row.pid, "SIGKILL");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
    }
  }
  return { status, signal, stdout, stderr, spawnError, timedOut, ms, observed, survivors, observationError };
}
