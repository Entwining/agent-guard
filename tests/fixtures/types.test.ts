import type { reasons } from "../../src/reasons";
import type { Tool } from "../../src/record";

export interface BehaviorRow {
  tool: Capitalize<Tool>;
  input: string;
  cwd: string;
  glob?: string;
  claude: 0 | 2;
  codex?: 0 | 2;
  codex_reason?: string;
  reason?: keyof typeof reasons;
  claude_suggestions?: (keyof typeof reasons)[];
  note?: string;
}
