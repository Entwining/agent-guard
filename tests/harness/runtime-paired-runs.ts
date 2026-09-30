import { runtimeCorpus } from "./runtime-corpus";
import { resultPath } from "./runtime-output";

interface Row {
  runtime: string;
  run: number;
  id: string;
  elapsedMs: number;
  verdict: string;
  expected: string;
  status: number;
  timedOut: boolean;
}
interface Report {
  ablated: boolean;
  manifest: { path: string; sha256: string }[];
  records: Row[];
}

const [normalPath, baselinePath, outputPath] = process.argv.slice(2);
if (!normalPath || !baselinePath || !outputPath) throw new Error("Usage: bun tests/harness/runtime-paired-runs.ts NORMAL.json BASELINE.json OUTPUT.json");
const output = resultPath(outputPath, "runtime-paired-runs");
const normal = (await Bun.file(normalPath).json()) as Report;
const baseline = (await Bun.file(baselinePath).json()) as Report;
if (normal.ablated || !baseline.ablated) throw new Error("Expected guarded normal and --ablate baseline reports");
if (JSON.stringify(normal.manifest) !== JSON.stringify(baseline.manifest)) throw new Error("Source manifests differ; latency comparison requires the same guard version");
if (normal.records.length !== baseline.records.length) throw new Error("Corpus sizes differ");
const key = (row: Pick<Row, "runtime" | "run" | "id">) => `${row.runtime}/${row.run}/${row.id}`;
const expectedKeys = ["pi", "claude"].flatMap((runtime) => [0, 1, 2].flatMap((run) => runtimeCorpus.map((fixture) => key({ runtime, run, id: fixture.id })))).sort();
for (const report of [normal, baseline]) {
  if (JSON.stringify(report.records.map(key).sort()) !== JSON.stringify(expectedKeys)) throw new Error("Expected each fixed corpus fixture exactly once per runtime/run");
}
const summary = [...new Set(normal.records.map((row) => row.runtime))].map((runtime) => {
  const rows = normal.records.filter((row) => row.runtime === runtime);
  const deltas = rows
    .map((row) => {
      const matches = baseline.records.filter((base) => base.runtime === runtime && base.run === row.run && base.id === row.id);
      const base = matches[0];
      if (matches.length !== 1 || !base || base.verdict !== "allow" || row.verdict !== row.expected || row.status !== 0 || base.status !== 0 || row.timedOut || base.timedOut)
        throw new Error(`Invalid latency pair: ${runtime}/${row.run}/${row.id}`);
      return row.elapsedMs - base.elapsedMs;
    })
    .sort((a, b) => a - b);
  return {
    runtime,
    pairs: deltas.length,
    runtimeDeltaP50Ms: deltas[Math.ceil(deltas.length * 0.5) - 1],
    runtimeDeltaP95Ms: deltas[Math.ceil(deltas.length * 0.95) - 1],
    negativeDeltas: deltas.filter((delta) => delta < 0).length,
  };
});
const report = {
  normalPath,
  baselinePath,
  method: "Sequential cold-start runtime elapsed wall-time subtraction paired by runtime/run/fixture; includes startup variance and different synthetic paths",
  summary,
};
await Bun.write(output, JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
