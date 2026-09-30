import { afterAll, describe, expect, test } from "bun:test";

import { runFault, type Fault, type FaultResult } from "./fault-injection";

const faults: Fault[] = ["slow-reason", "large-reason"];
const results: (FaultResult & { runIndex: number })[] = [];

afterAll(() => {
  console.log(
    "Fault injection results:",
    JSON.stringify(
      results.map(({ fault, runIndex, exit, signalCode, milliseconds, descendantsAliveAtReturn, producerBytesWritten, producerControl, stdout, stderr, entrySnapshotSha256, runnerEvidence }) => ({
        fault,
        runIndex,
        exit,
        signalCode,
        milliseconds: Math.round(milliseconds),
        descendantsAliveAtReturn,
        producerBytesWritten,
        producerControl,
        stdoutLength: stdout.length,
        stderrLength: stderr.length,
        stderrXCount: (stderr.match(/x/g) ?? []).length,
        stderrNewlineCount: (stderr.match(/\n/g) ?? []).length,
        reason: stderr.slice(0, 160).replaceAll("\n", " "),
        entrySnapshotSha256,
        runnerEvidence,
      })),
    ),
  );
});

describe("Synthetic runner fault injection", () => {
  test.each(faults)(
    "%s",
    async (fault) => {
      const expectedReason: Record<Fault, string> = {
        "slow-reason": "guard failed",
        "large-reason": "xxxxxxxxxxxxxxxx",
      };
      const failures: string[] = [];
      const verdicts: number[] = [];
      for (let runIndex = 1; runIndex <= 3; runIndex++) {
        const result = await runFault(fault);
        verdicts.push(result.exit);
        results.push({ ...result, runIndex });
        const prefix = `run ${runIndex}`;
        if (result.exit !== 2) failures.push(`${prefix}: expected exit 2, received ${result.exit} (${result.signalCode ?? "no signal"})`);
        if (result.milliseconds >= 4_500) failures.push(`${prefix}: latency ${Math.round(result.milliseconds)}ms exceeded 4500ms`);
        if (!result.stderr.includes(expectedReason[fault])) failures.push(`${prefix}: stderr did not contain expected reason ${JSON.stringify(expectedReason[fault])}`);
        if (fault === "large-reason") {
          if (result.producerBytesWritten !== 131_073) failures.push(`${prefix}: entry-path producer wrote ${result.producerBytesWritten ?? "no complete count"} bytes, expected 131073`);
          if (JSON.stringify(result.producerControl) !== JSON.stringify({ bytesWritten: 131_073, outputLength: 131_073, outputMatchesPayload: true })) {
            failures.push(`${prefix}: direct producer control did not reproduce 131073 exact payload bytes: ${JSON.stringify(result.producerControl)}`);
          }
          if (result.stderr.length !== 131_073) failures.push(`${prefix}: expected 131073 stderr bytes, received ${result.stderr.length}`);
        }
        if (result.descendantsAliveAtReturn.length > 0) failures.push(`${prefix}: descendants survived at return: ${result.descendantsAliveAtReturn.join(",")}`);
      }
      if (new Set(verdicts).size > 1) failures.push(`verdict changed across three runs: ${verdicts.join(",")}`);
      expect(failures).toEqual([]);
    },
    30_000,
  );
});
