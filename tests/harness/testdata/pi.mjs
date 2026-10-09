import { spawnSync } from "node:child_process";
export default function (pi) {
  pi.registerProvider("harness", { baseUrl: process.env.AGENT_GUARD_TEST_MODEL_URL, apiKey: "synthetic-not-a-credential", api: "anthropic-messages", models: [{ id: "synthetic", name: "Synthetic", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 200000, maxTokens: 4096 }] });
  pi.on("tool_call", (event, ctx) => {
    if (event.toolName !== "bash") return;
    const input = { tool_name: "bash", tool_input: event.input, cwd: ctx.cwd };
    const result = spawnSync(process.env.AGENT_GUARD_TEST_HELPER, ["--hook", "pi"], { cwd: ctx.cwd, input: JSON.stringify(input), encoding: "utf8", timeout: 5000 });
    if (result.status === 0) return;
    return { block: true, reason: result.stderr?.trim() || result.error?.message || "agent-guard failed" };
  });
}
