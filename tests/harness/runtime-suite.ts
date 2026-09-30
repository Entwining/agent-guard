import { mkdtemp, mkdir, writeFile, readFile, rm, cp, symlink, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";

import { runtimeCorpus } from "./runtime-corpus";
import { resultPath } from "./runtime-output";

const root = resolve(import.meta.dir, "../..");
const pairedIndex = process.argv.indexOf("--paired");
if (pairedIndex >= 0 && !process.argv[pairedIndex + 1]) throw new Error("--paired requires a baseline output path");
const normalOutput = resultPath(process.argv[2]?.startsWith("--") ? undefined : process.argv[2], "runtime-suite");
const baselineOutput = pairedIndex >= 0 ? resultPath(process.argv[pairedIndex + 1], "runtime-suite-baseline") : undefined;
const home = await mkdtemp(join(tmpdir(), "guard-runtime-"));
const workspace = join(home, "workspace");
interface ToolResult {
  type: string;
  is_error?: boolean;
  content: unknown;
}
interface HookResult {
  input: { tool_input: { command: string } };
  status: number | null;
  stderr: string;
  stdout: string;
  signal: string | null;
  spawnError?: string;
  timedOut: boolean;
  ms: number;
  observed: { pid: number; ppid: number; started: string }[];
  survivors: { pid: number; ppid: number; started: string }[];
  observationError?: string;
}
interface RecordRow {
  runtime: string;
  run: number;
  id: string;
  command: string;
  expected: string;
  verdict: string;
  runtimeResult: ToolResult | undefined;
  hooks: HookResult[];
  elapsedMs: number;
  status: number;
  timedOut: boolean;
  stdout: string;
  stderr: string;
}
const received: { runtime: string; command: string; result: ToolResult }[] = [];
const modes = pairedIndex >= 0 ? [false, true] : [process.argv.includes("--ablate")];
let active: { command: string; runtime: string };
const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  async fetch(req) {
    if (req.method !== "POST") return new Response(null, { status: 200 });
    const body = (await req.json()) as { model: string; stream: boolean; messages: { content: ToolResult[] | string }[] };
    if (!req.url.includes("/messages")) return Response.json({ input_tokens: 10 });
    const last = body.messages?.at(-1);
    const result = Array.isArray(last?.content) ? last.content.find((x) => x.type === "tool_result") : undefined;
    if (result) received.push({ runtime: active.runtime, command: active.command, result });
    const content = result ? { type: "text", text: "done" } : { type: "tool_use", id: "tool_harness", name: active.runtime === "claude" ? "Bash" : "bash", input: { command: active.command } };
    const stop = result ? "end_turn" : "tool_use";
    const message = {
      id: "msg_harness",
      type: "message",
      role: "assistant",
      model: body.model,
      content: [content],
      stop_reason: stop,
      stop_sequence: null,
      usage: { input_tokens: 10, output_tokens: 10 },
    };
    if (!body.stream) return Response.json(message);
    const events = [
      ["message_start", { type: "message_start", message: { ...message, content: [], stop_reason: null } }],
      ["content_block_start", { type: "content_block_start", index: 0, content_block: content.type === "tool_use" ? { ...content, input: {} } : { type: "text", text: "" } }],
      [
        "content_block_delta",
        {
          type: "content_block_delta",
          index: 0,
          delta: content.type === "tool_use" ? { type: "input_json_delta", partial_json: JSON.stringify(content.input) } : { type: "text_delta", text: content.text },
        },
      ],
      ["content_block_stop", { type: "content_block_stop", index: 0 }],
      ["message_delta", { type: "message_delta", delta: { stop_reason: stop, stop_sequence: null }, usage: { output_tokens: 10 } }],
      ["message_stop", { type: "message_stop" }],
    ];
    return new Response(events.map(([event, data]) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`).join(""), { headers: { "content-type": "text/event-stream" } });
  },
});

try {
  await mkdir(workspace, { recursive: true });
  const installation = join(home, "installation");
  await mkdir(installation);
  for (const file of ["bin", "src", "tsconfig.json", "package.json"]) await cp(join(root, file), join(installation, file), { recursive: true });
  await symlink(join(root, "node_modules"), join(installation, "node_modules"));
  const manifest: { path: string; sha256: string }[] = [];
  async function digest(directory: string) {
    for (const entry of await readdir(join(installation, directory), { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await digest(path);
      else manifest.push({ path, sha256: new Bun.CryptoHasher("sha256").update(await readFile(join(installation, path))).digest("hex") });
    }
  }
  await digest("src");
  await digest("bin");
  for (const role of ["runner", "guard"]) {
    const path = join(installation, "src", `${role}.ts`);
    const observer = `import {appendFileSync as recordHarnessPid} from "node:fs"; recordHarnessPid(process.env["AGENT_GUARD_TEST_PID_TRACE"]!, JSON.stringify({pid:process.pid,ppid:process.ppid,started:String(Date.now()),role:${JSON.stringify(role)}})+"\\n");\n`;
    await writeFile(path, observer + (await readFile(path, "utf8")));
  }
  await mkdir(join(home, "Library/Containers/harness"), { recursive: true });
  await mkdir(join(home, ".ssh"));
  await mkdir(join(home, ".claude"));
  await writeFile(join(workspace, "public.txt"), "PUBLIC_CANARY\n");
  await writeFile(join(home, "Library/Containers/harness/canary.txt"), "APP_DATA_CANARY\n");
  await writeFile(join(home, ".ssh/id_ed25519"), "SYNTHETIC_CANARY_NOT_A_KEY\n");
  const hook = `${process.execPath} ${join(import.meta.dir, "runtime-hook-driver.ts")}`;
  await writeFile(join(home, ".claude/settings.json"), JSON.stringify({ hooks: { PreToolUse: [{ matcher: "Bash", hooks: [{ type: "command", command: hook, timeout: 5 }] }] } }));
  const env = {
    HOME: home,
    PATH: process.env["PATH"]!,
    TMPDIR: home,
    AGENT_GUARD_TEST_ENTRY: join(installation, "bin/agent-guard"),
    AGENT_GUARD_TEST_PID_TRACE: join(home, "pids.jsonl"),
    AGENT_GUARD_TEST_MODEL_URL: server.url.toString().replace(/\/$/, ""),
    ANTHROPIC_BASE_URL: server.url.toString().replace(/\/$/, ""),
    ANTHROPIC_API_KEY: "synthetic-not-a-credential",
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    CLAUDE_CONFIG_DIR: join(home, ".claude"),
  };
  for (const ablated of modes) {
    const records: RecordRow[] = [];
    for (const runtime of ["pi", "claude"]) {
      for (let run = 0; run < 3; run++)
        for (const fixture of runtimeCorpus) {
          active = { command: fixture.command, runtime };
          const trace = join(home, "trace.jsonl");
          await writeFile(trace, "");
          await writeFile(env.AGENT_GUARD_TEST_PID_TRACE, "");
          const responseOffset = received.length;
          const args =
            runtime === "pi"
              ? [
                  "pi",
                  "--no-extensions",
                  "--no-session",
                  "-e",
                  join(import.meta.dir, "runtime-pi.ts"),
                  "--provider",
                  "harness",
                  "--model",
                  "synthetic",
                  "--mode",
                  "json",
                  "-p",
                  "Run the requested tool.",
                ]
              : ["claude", "-p", "Run the requested tool.", "--model", "claude-sonnet-4-5", "--output-format", "stream-json", "--verbose", "--dangerously-skip-permissions"];
          const start = performance.now();
          const child = Bun.spawn(args, { cwd: workspace, env: { ...env, AGENT_GUARD_TEST_HOOK_TRACE: trace, AGENT_GUARD_TEST_ABLATE: ablated ? "1" : "0" }, stdout: "pipe", stderr: "pipe" });
          let timedOut = false;
          const timer = setTimeout(() => {
            timedOut = true;
            child.kill("SIGKILL");
          }, 20000);
          const [stdout, stderr, status] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
          clearTimeout(timer);
          const hooks = (await readFile(trace, "utf8"))
            .trim()
            .split("\n")
            .filter(Boolean)
            .map((x) => JSON.parse(x) as HookResult);
          const runtimeResult = received.slice(responseOffset).findLast((x) => x.runtime === runtime && x.command === fixture.command)?.result;
          const contents = JSON.stringify(runtimeResult?.content);
          const hookResult = hooks[0];
          const denied =
            hookResult &&
            hookResult.status !== 0 &&
            hookResult.stderr.trim().length > 0 &&
            runtimeResult?.is_error === true &&
            contents?.includes(JSON.stringify(hookResult.stderr.trim()).slice(1, -1)) &&
            !contents.includes("APP_DATA_CANARY") &&
            !contents.includes("SYNTHETIC_CANARY_NOT_A_KEY");
          const executed = contents?.includes(fixture.witness);
          const verdict = hooks.length === 1 && hookResult?.input.tool_input.command === fixture.command && runtimeResult ? (denied ? "deny" : executed ? "allow" : "unverified") : "unverified";
          records.push({
            runtime,
            run,
            id: fixture.id,
            command: fixture.command,
            expected: fixture.expected,
            verdict,
            runtimeResult,
            hooks,
            elapsedMs: performance.now() - start,
            status,
            timedOut,
            stdout,
            stderr,
          });
          console.log(JSON.stringify({ runtime, run, id: fixture.id, verdict, hooks: hooks.length, status }));
        }
    }
    const percentile = (a: number[], p: number) => (a.length ? a.sort((x, y) => x - y)[Math.ceil(a.length * p) - 1] : null);
    const summary = ["pi", "claude"].map((runtime) => {
      const rows = records.filter((x) => x.runtime === runtime);
      const ms = rows.flatMap((x) => x.hooks.map((h) => h.ms));
      return {
        runtime,
        calls: rows.length,
        verified: rows.filter((x) => x.verdict !== "unverified").length,
        entryStatusViolations: rows.flatMap((x) => x.hooks).filter((h) => h.status !== 0 && h.status !== 2).length,
        falseDeny: rows.filter((x) => x.expected === "allow" && x.verdict === "deny").length,
        falseAllow: rows.filter((x) => x.expected === "deny" && x.verdict === "allow").length,
        overheadP50Ms: percentile(ms, 0.5),
        overheadP95Ms: percentile(ms, 0.95),
        deterministic: rows.some((r) => r.verdict === "unverified") ? null : runtimeCorpus.every((f) => new Set(rows.filter((x) => x.id === f.id).map((x) => x.verdict)).size === 1),
        observedProcesses: rows.reduce((n, r) => n + r.hooks.reduce((m, h) => m + h.observed.length, 0), 0),
        trackedSurvivors:
          rows.some((r) => r.hooks.length !== 1) || rows.flatMap((r) => r.hooks).some((h) => h.observationError)
            ? null
            : rows.reduce((n, r) => n + r.hooks.reduce((m, h) => m + h.survivors.length, 0), 0),
        observationErrors: rows.flatMap((r) => r.hooks.filter((h) => h.observationError)).length,
      };
    });
    const output = ablated && baselineOutput ? baselineOutput : normalOutput;
    await writeFile(
      output,
      JSON.stringify(
        {
          syntheticHome: home,
          ablated,
          manifest,
          measurement: "Guard process wall time; adapter/runtime overhead and paired no-hook baseline not measured",
          observerAddition: "After canonical manifest: append synthetic own PID metadata at runner.ts and guard.ts module entry; bin/agent-guard remains byte-identical",
          survivalCoverage: "Own entry PID plus synthetic runner/guard PID trace, kill(pid,0) after hook; watchdog and unlogged descendants unverified",
          summary,
          records,
        },
        null,
        2,
      ),
    );
    console.log(JSON.stringify(summary));
    process.exitCode =
      records.every((r) => r.status === 0 && !r.timedOut && r.verdict === (ablated ? "allow" : r.expected)) &&
      summary.every((s) => s.trackedSurvivors === 0 && s.observationErrors === 0 && s.entryStatusViolations === 0)
        ? process.exitCode
        : 1;
  }
} finally {
  server.stop(true);
  await rm(home, { recursive: true, force: true });
}
