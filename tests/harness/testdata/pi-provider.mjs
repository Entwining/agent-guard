export default function (pi) {
  pi.registerProvider("harness", { baseUrl: process.env.AGENT_GUARD_TEST_MODEL_URL, apiKey: "synthetic-not-a-credential", api: "anthropic-messages", models: [{ id: "synthetic", name: "Synthetic", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 200000, maxTokens: 4096 }] });
}
