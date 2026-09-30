// Parse shell structure into command records. Failed syntax blocks the call;
// interpreter code still reaches the signature rules.

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

import type { Assign, BinaryCmd, CallExpr, CmdSubst, DeclClause, FuncDecl, IfClause, Lit, Node, ParamExp, Redirect as RedirectNode, Stmt, Subshell, Word as WordNode } from "mvdan-sh";
import sh from "mvdan-sh";

import { expandHome } from "../filesystem/paths";
import type { Command, Redirect, Script, Word } from "../record";
import { resolveCommand, stdinKind } from "./argv";
import { misreadComment } from "./comments";
import { boundedDirectories, changeDirectory } from "./cwd";
import { movedOnSuccess } from "./moves";
import { markWalkedInput, shellInput, xargsHereInput, xargsReplacements } from "./pipeline";
import { readWord } from "./words";

const { syntax } = sh;
let packageDir = import.meta.dir;
let parserPath = "";
while (!parserPath) {
  const candidate = join(packageDir, "node_modules/mvdan-sh/index.js");
  if (existsSync(candidate)) parserPath = candidate;
  else {
    const parent = dirname(packageDir);
    if (parent === packageDir) throw new Error("mvdan-sh dependency is missing");
    packageDir = parent;
  }
}
if (sh !== (await import(pathToFileURL(parserPath).href)).default) throw new Error("mvdan-sh resolved outside its dependency package");
const parser = syntax.NewParser(syntax.KeepComments(true), syntax.Variant(syntax.LangBash));

interface Scope {
  // Shared by the commands a cd in this scope moves. `failures` collects where failed cds in an && chain leave the shell, for the commands after it.
  readonly dir: { cwd: string; alternatives?: string[] | undefined; failures?: string[] | undefined };
  readonly vars: Map<string, string>;
  readonly slice: (start: number, end: number) => string;
}

const type = (node: Node) => syntax.NodeType(node);

// A function call runs the body in the caller's shell; bound the work a chain of
// calls can multiply.
const maxFunctionRuns = 256;

export function parseScript(source: string, cwd: string, home: string): Script {
  const script: Script = { commands: [], uninspectable: [], parseFailed: false };

  function parse(src: string, cwd: string) {
    const bytes = Buffer.from(src);
    let file;
    try {
      file = parser.Parse(src, "");
    } catch {
      file = undefined;
    }
    if (!file || misreadComment(file, bytes)) {
      script.parseFailed = true;
      return;
    }
    const slice = (start: number, end: number) => bytes.subarray(start, end).toString();
    const scope: Scope = { dir: { cwd }, vars: new Map(), slice };
    for (const stmt of file.Stmts) statement(stmt!, scope);
  }

  const functions = new Map<string, { body: Stmt; slice: Scope["slice"] }>();
  const running = new Set<string>();
  let functionRuns = 0;

  const text = (node: Node, scope: Scope) => scope.slice(node.Pos().Offset(), node.End().Offset());
  // A cd inside runs in a separate shell and leaves this scope's cwd alone.
  const isolated = (scope: Scope): Scope => ({ ...scope, dir: { ...scope.dir, failures: undefined }, vars: new Map(scope.vars) });

  function statement(stmt: Stmt, outer: Scope) {
    const scope = stmt.Background ? isolated(outer) : outer;
    const cmd = stmt.Cmd;
    const kind = cmd && type(cmd);
    if (kind === "CallExpr" || kind === "DeclClause") return simple(cmd, stmt.Redirs, scope);
    // Redirections on a compound command, or with no command at all, still
    // open their files.
    if (stmt.Redirs.length) simple(null, stmt.Redirs, scope);
    if (!cmd) return;
    if (kind === "BinaryCmd") return binary(cmd as BinaryCmd, scope);
    if (kind === "Subshell") {
      const inner = isolated(scope);
      for (const s of (cmd as Subshell).Stmts) statement(s!, inner);
      return;
    }
    children(cmd, scope);
  }

  // Only the left side of && moves the directory the right side runs in.
  function binary(cmd: BinaryCmd, scope: Scope) {
    const op = scope.slice(cmd.OpPos.Offset(), cmd.OpPos.Offset() + 2).trim();
    if (op === "&&" && movedOnSuccess(cmd.X!, scope.slice)) {
      // The right side runs only where every cd before it succeeded. A compound right side may run a command after its own failed cd.
      const outer = scope.dir.failures;
      const failures: string[] = [];
      scope.dir.failures = failures;
      statement(cmd.X!, scope);
      scope.dir.failures = cmd.Y!.Cmd && type(cmd.Y!.Cmd) === "CallExpr" ? failures : undefined;
      statement(cmd.Y!, scope);
      scope.dir.failures = outer;
      if (outer) outer.push(...failures);
      else scope.dir.alternatives = boundedDirectories(scope.dir.cwd, [...(scope.dir.alternatives ?? []), ...failures], home);
      return;
    }
    const failed = isolated(scope);
    const start = script.commands.length;
    statement(cmd.X!, op === "&&" || op === "||" ? scope : isolated(scope));
    const middle = script.commands.length;
    // zsh runs the last element of a pipeline in this shell and bash forks it,
    // so a cd there may or may not move the commands after it.
    const right = op === "||" ? failed : op === "&&" ? scope : isolated(scope);
    statement(cmd.Y!, right);
    if (op === "|" || op === "|&") {
      const left = script.commands.slice(start, middle);
      const rest = script.commands.slice(middle);
      markWalkedInput(left, rest);
      for (const item of [...xargsReplacements(left, rest), ...shellInput(left, rest)]) parse(item.source, item.cwd);
    }
    if (op !== "&&") scope.dir.alternatives = boundedDirectories(scope.dir.cwd, [...(scope.dir.alternatives ?? []), right.dir.cwd, ...(right.dir.alternatives ?? [])], home);
  }

  // Compound words may hold substitutions; branch bodies may not run.
  function children(node: Node, scope: Scope) {
    if (type(node) === "FuncDecl") {
      const func = node as FuncDecl;
      if (func.Name && func.Body) functions.set(func.Name.Value, { body: func.Body, slice: scope.slice });
      scope = isolated(scope);
    }
    if (type(node) === "IfClause") return conditional(node as IfClause, scope);
    const outer = scope;
    const loop = type(node) === "WhileClause" || type(node) === "ForClause";
    if (loop) scope = isolated(scope);
    syntax.Walk(node, (child) => {
      if (!child || child === node) return true;
      const kind = type(child);
      if (kind === "Stmt") statement(child as Stmt, scope);
      else if (kind === "Word") {
        const value = word(child as WordNode, scope);
        for (const cwd of [scope.dir.cwd, ...(scope.dir.alternatives ?? [])]) {
          script.commands.push({ argv: [value], redirects: [], cwd, program: -1, wrappers: [], shell: true, flags: new Set() });
        }
      } else return true;
      return false;
    });
    if (loop) outer.dir.alternatives = boundedDirectories(outer.dir.cwd, [...(outer.dir.alternatives ?? []), scope.dir.cwd, ...(scope.dir.alternatives ?? [])], home);
  }

  function conditional(node: IfClause, scope: Scope) {
    const condition = isolated(scope);
    for (const stmt of node.Cond) statement(stmt!, condition);
    const then = isolated(condition);
    for (const stmt of node.Then) statement(stmt!, then);
    const branches = [scope, condition, then];
    if (node.Else) {
      const otherwise = isolated(condition);
      conditional(node.Else, otherwise);
      branches.push(otherwise);
    }
    scope.dir.alternatives = boundedDirectories(
      scope.dir.cwd,
      branches.flatMap((branch) => [branch.dir.cwd, ...(branch.dir.alternatives ?? [])]),
      home,
    );
  }

  function expansions(node: Node, scope: Scope, vars: string[]) {
    syntax.Walk(node, (child) => {
      if (!child) return true;
      const kind = type(child);
      if (kind === "ParamExp") vars.push((child as ParamExp).Param!.Value);
      if (kind !== "CmdSubst" && kind !== "ProcSubst") return true;
      const inner = isolated(scope);
      for (const s of (child as CmdSubst).Stmts) statement(s!, inner);
      return false;
    });
  }

  function word(node: WordNode, scope: Scope): Word {
    return readWord(node, scope.slice, scope.vars, home, scope.dir.cwd, (part, names) => expansions(part, scope, names));
  }

  function assign(node: Assign, scope: Scope, role: "assign" | "arg"): Word {
    const name = node.Name?.Value ?? "";
    const value = node.Value ? word(node.Value, scope) : undefined;
    // Array and index expressions can hold substitutions outside the value.
    syntax.Walk(node, (child) => {
      if (!child || child === node) return true;
      if (child === node.Value) return false;
      if (type(child) === "Word") word(child as WordNode, scope);
      return type(child) !== "Word";
    });
    return {
      ...(value ?? { expands: false, globs: false, vars: [], pwd: false }),
      text: `${name}=${value?.text ?? ""}`,
      raw: text(node, scope),
      role,
      value: `${name}=${value?.text ?? ""}`,
    };
  }

  function literal(value: string): Word {
    return { text: value, raw: value, expands: false, globs: false, vars: [], role: "arg", value, pwd: false };
  }

  function simple(cmd: Node | null, redirs: (RedirectNode | null)[], scope: Scope) {
    const argv: Word[] = [];
    if (cmd && type(cmd) === "CallExpr") {
      for (const a of (cmd as CallExpr).Assigns) argv.push(assign(a!, scope, "assign"));
      for (const w of (cmd as CallExpr).Args) argv.push(word(w!, scope));
    } else if (cmd) {
      const decl = cmd as DeclClause;
      argv.push(literal(decl.Variant!.Value));
      for (const a of decl.Args) {
        if (!a!.Naked) argv.push(assign(a!, scope, "arg"));
        else argv.push(a!.Value ? word(a!.Value, scope) : literal(a!.Name?.Value ?? ""));
      }
    }
    const redirects = redirs.map((r) => redirect(r!, scope)).filter((r) => r !== undefined);
    const command: Command = {
      argv,
      redirects,
      cwd: scope.dir.cwd,
      program: -1,
      wrappers: [],
      shell: true,
      flags: new Set(),
    };
    script.commands.push(command);
    const { children: sources, code } = resolveCommand(command, home);
    const moved = (cwd: string) => command.argv.map((w) => (w.pwd ? { ...w, text: w.text.replaceAll(scope.dir.cwd, cwd), value: w.value.replaceAll(scope.dir.cwd, cwd) } : w));
    for (const cwd of scope.dir.alternatives ?? []) script.commands.push({ ...command, cwd, argv: moved(cwd) });
    for (const { source, items } of sources) {
      for (const cwd of [command.cwd, ...(scope.dir.alternatives ?? [])]) {
        const start = script.commands.length;
        parse(source, cwd);
        if (items) for (const child of script.commands.slice(start)) child.items ??= items;
      }
    }
    for (const text of code) script.uninspectable.push({ text, cwd: command.cwd });
    for (const r of redirects) {
      if (r.direction !== "heredoc" && r.direction !== "herestring") continue;
      const stdin = stdinKind(command);
      if (stdin === "shell") parse(r.target, command.cwd);
      if (stdin === "code") script.uninspectable.push({ text: r.target, cwd: command.cwd });
      if (command.wrappers.includes("xargs")) for (const item of xargsHereInput(command, r.target)) parse(item.source, item.cwd);
    }
    const called = command.wrappers.every((w) => w === "time") ? command.argv[command.program]?.text : undefined;
    const func = called === undefined ? undefined : functions.get(called);
    if (func && !running.has(called!)) {
      if (++functionRuns > maxFunctionRuns) script.parseFailed = true;
      else {
        running.add(called!);
        // The body's commands run after its own failed cds.
        const failures = scope.dir.failures;
        scope.dir.failures = undefined;
        statement(func.body, { ...scope, slice: func.slice });
        scope.dir.failures = failures;
        running.delete(called!);
      }
    }
    track(command, scope);
  }

  function redirect(node: RedirectNode, scope: Scope): Redirect | undefined {
    const op = scope.slice(node.OpPos.Offset(), node.Word!.Pos().Offset()).trim();
    if (op === "<<" || op === "<<-") {
      const quoted = node.Word!.Parts.some((p) => type(p) !== "Lit" || (p as Lit).Value.includes("\\"));
      const vars: string[] = [];
      if (node.Hdoc && !quoted) expansions(node.Hdoc, scope, vars);
      return { direction: "heredoc", target: node.Hdoc ? text(node.Hdoc, scope) : "", globs: false, expands: false, vars };
    }
    const target = word(node.Word!, scope);
    if (op === "<<<") return { direction: "herestring", target: target.text, globs: false, expands: false, vars: target.vars };
    if ((op === "<&" || op === ">&") && /^(\d+|-)$/.test(target.text)) return undefined;
    const direction = op === "<" || op === "<>" ? "in" : "out";
    return { direction, target: target.text, globs: target.globs, expands: target.expands, vars: [] };
  }

  function track(command: Command, scope: Scope) {
    const program = command.argv[command.program];
    const declaration = command.shell && ["export", "local", "declare", "typeset"].includes(program?.text ?? "");
    for (const word of command.argv.filter((w) => ((command.program < 0 && w.role === "precommand") || (declaration && w.role === "arg")) && /^[A-Za-z_][A-Za-z0-9_]*=/.test(w.text))) {
      const at = word.text.indexOf("=");
      if (!word.expands) scope.vars.set(word.text.slice(0, at), expandHome(word.text.slice(at + 1), home));
    }
    if (!program || !command.shell || !["cd", "pushd"].includes(program.text)) return;
    const args = command.argv.slice(command.program + 1);
    let target = args.find((w) => !w.text.startsWith("-"))?.text;
    if (target === undefined && args.every((w) => /^(--|-[PLqs]+)$/.test(w.text)) && program.text === "cd") target = home;
    if (target !== undefined) {
      // Bash obeys the last of -L and -P, zsh any -P; keep both possibilities when they disagree. Unresolved operands retain lexical inference.
      const operand = args.findIndex((w) => !w.text.startsWith("-"));
      const modes = args
        .slice(0, operand < 0 ? args.length : operand)
        .filter((w) => /^-[A-Za-z]+$/.test(w.text))
        .flatMap((w) => [...w.text].filter((c) => c === "L" || c === "P"));
      const physical = modes.includes("P") && !args[operand]?.expands && !args[operand]?.globs;
      const disputed = physical && modes.at(-1) === "L";
      const move = (cwd: string) => [changeDirectory(cwd, target, physical), ...(disputed ? [changeDirectory(cwd, target, false)] : [])];
      const [next, ...others] = move(scope.dir.cwd);
      const stayed = [scope.dir.cwd, ...(scope.dir.alternatives ?? [])];
      scope.dir.failures?.push(...stayed);
      scope.dir.alternatives = boundedDirectories(next!, [...(scope.dir.failures ? [] : stayed), ...others, ...(scope.dir.alternatives ?? []).flatMap(move)], home);
      scope.dir.cwd = next!;
    }
  }

  // `.` segments mark physical cd spellings, so the hook's directory must not bring its own; they name no step in either reading.
  parse(
    source,
    cwd
      .split("/")
      .filter((segment) => segment !== ".")
      .join("/") || "/",
  );
  return script;
}
