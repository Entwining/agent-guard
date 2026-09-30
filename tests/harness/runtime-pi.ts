import { appendFileSync } from "node:fs";

import { observeGuard } from "./runtime-observation";

interface PiAPI {
  registerProvider(name: string, options: unknown): void;
  on(name: string, handler: (event: { toolName: string; input: Record<string, unknown> }, ctx: { cwd: string }) => Promise<{ block: true; reason: string } | undefined>): void;
}

export default function (pi: PiAPI) {
  pi.registerProvider("harness", {
    baseUrl: process.env["AGENT_GUARD_TEST_MODEL_URL"],
    apiKey: "synthetic-not-a-credential",
    api: "anthropic-messages",
    models: [{ id: "synthetic", name: "Synthetic", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 200000, maxTokens: 4096 }],
  });
  pi.on("tool_call", async (event, ctx) => {
    if (event.toolName !== "bash") return;
    const input = { tool_name: "bash", tool_input: event.input, cwd: ctx.cwd };
    const result = await observeGuard("pi", JSON.stringify(input), ctx.cwd);
    appendFileSync(process.env["AGENT_GUARD_TEST_HOOK_TRACE"]!, JSON.stringify({ input, ...result }) + "\n");
    if (result.status === 0) return;
    return { block: true, reason: result.stderr.trim() || result.spawnError || "agent-guard failed" };
  });
}
