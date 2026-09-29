// A search option's value keeps the option's word as option:ROLE.
export type ValueRole = "pattern" | "patfile" | "glob" | "nglob" | "optarg";
export type Role = "arg" | "assign" | "precommand" | "namespace" | "program" | "code" | "option" | "path" | ValueRole | `option:${ValueRole}`;
export type Flag = "explicit" | "files" | "help" | "fixed" | "recursive" | "include" | "replace" | "hidden";

export interface Word {
  text: string; // quotes removed; ~, $HOME and ${HOME} expanded
  raw: string;
  expands: boolean; // holds an expansion the front end cannot resolve
  globs: boolean; // holds an unquoted glob character
  vars: string[]; // parameter names the shell expands
  role: Role; // set by argv.ts
  value: string; // an option's value without its flag, set by argv.ts
  pwd: boolean; // holds $PWD, $(pwd) or ~+, expanded to the directory the command was read in
}

export interface Redirect {
  direction: "in" | "out" | "herestring" | "heredoc";
  target: string; // file, word, or heredoc body
  globs: boolean; // the target holds an unquoted glob character
  vars: string[]; // parameters the shell expands in the target or body
}

export type Effect = "read" | "write" | "list" | "meta" | "use" | "enter" | "name";

// The file names a walk hands to a command through find -exec, fd -x or xargs.
export interface Items {
  root: string;
  hidden: boolean;
}

// A path a command touches, with what the command does to it.
export interface Target {
  path: string; // absolute; ~, $HOME and $PWD expanded when unquoted, resolved against the command's cwd or a tar -C base
  unresolved: string; // the same path before `..` is folded, for the symlink walk
  glob: boolean; // the shell expands it before the program runs
  effect: Effect;
  walk: "none" | "visible" | "hidden"; // a read or list of a directory reaches what is under it; hidden includes dotfiles
  sends: boolean; // the program transmits what it reads
  expands: boolean; // the word held an expansion the front end could not resolve
  via: "tool" | "operand" | "option" | "redirect" | "items" | "cwd" | "scan" | "code";
  search: boolean; // root of a content search
  command: number; // index of the command in Request.commands, -1 for a tool request
}

export interface Command {
  argv: Word[];
  redirects: Redirect[];
  cwd: string; // after the cd commands that run before it in the same shell
  program: number; // argv index of the word that runs, -1 when nothing runs
  wrappers: string[];
  shell: boolean; // the program runs in this shell rather than behind a wrapper
  flags: Set<Flag>;
  items?: Items;
}

// Text the front end cannot structure into commands, such as the code an interpreter runs.
export interface Fragment {
  text: string;
  cwd: string;
}

export interface Script {
  commands: Command[];
  uninspectable: Fragment[];
  parseFailed: boolean;
}

export type Runtime = "claude" | "codex" | "pi";
export type Tool = "bash" | "read" | "edit" | "write" | "grep";

export interface Request {
  runtime: Runtime;
  tool: Tool;
  home: string;
  cwd: string;
  inputCwd: string;
  pathInput: string;
  operation: "" | "read" | "write" | "search";
  target: string;
  searchRoot: string;
  glob: string;
  commands: Command[];
  uninspectable: Fragment[];
  parseFailed: boolean;
}
