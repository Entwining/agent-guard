#!/usr/bin/env python3
"""Measure one macOS guard/OS/form cell; synthetic validation never measures TCC."""

import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import plistlib
import secrets
import shlex
import shutil
import signal
import subprocess
import sys
import time

FORMS = ("direct_operand", "inline_literal", "external_script", "runtime_config_path")
DEADLINE = 20
WRITER = r'''#import <Foundation/Foundation.h>
int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSString *home = NSHomeDirectory();
        if (argc != 2 || ![home isEqualToString:[NSString stringWithUTF8String:argv[1]]]) {
            fprintf(stderr, "Canary owner sandbox unavailable\n");
            return 2;
        }
        NSError *error = nil;
        NSData *data = [NSData dataWithContentsOfURL:[[NSBundle mainBundle] URLForResource:@"canary" withExtension:@"bin"]];
        NSString *target = [home stringByAppendingPathComponent:@"canary.bin"];
        if (!data || ![data writeToFile:target options:NSDataWritingWithoutOverwriting error:&error]) {
            fprintf(stderr, "Canary creation failed\n");
            return 1;
        }
        NSData *receipt = [NSJSONSerialization dataWithJSONObject:@{@"target":target, @"bytes_written":@([data length])} options:0 error:&error];
        fwrite([receipt bytes], 1, [receipt length], stdout);
        return 0;
    }
}
'''


def result_path(value, real_home, checkout):
    output = Path(os.path.abspath(os.path.expanduser(str(value))))
    protected = tuple(real_home / name for name in ("Library", "Desktop", "Documents", "Downloads"))

    def forbidden(path):
        return path.is_relative_to(checkout) or any(path.is_relative_to(root) for root in protected)

    if forbidden(output):
        raise ValueError("output must stay outside the checkout and protected user folders")

    # Inspect links one component at a time; never inspect children of a protected target.
    pending = list(output.parts[1:])
    current = Path(output.anchor)
    links = 0
    while pending:
        current /= pending.pop(0)
        if forbidden(current):
            raise ValueError("output must stay outside the checkout and protected user folders")
        if current.is_symlink():
            links += 1
            if links > 40:
                raise ValueError("too many output path links")
            target = Path(os.readlink(current))
            linked = target if target.is_absolute() else current.parent / target
            normalized = Path(os.path.abspath(linked))
            if forbidden(normalized):
                raise ValueError("output must stay outside the checkout and protected user folders")
            pending = list(normalized.parts[1:]) + pending
            current = Path(normalized.anchor)
    return current


def run(command, *, env=None, stdin=None, deadline=DEADLINE):
    """Own the subprocess group so a timeout also terminates its descendants."""
    start = time.monotonic()
    result = {"command": shlex.join(map(str, command)), "argv": list(map(str, command)), "deadline_seconds": deadline}
    try:
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, start_new_session=True)
    except OSError as error:
        return result | {"operation_result": "launch_error", "exit_code": None, "stderr": str(error), "stdout": b""}
    try:
        out, err = child.communicate(stdin, timeout=deadline)
        status = "success" if child.returncode == 0 else "nonzero_exit"
    except subprocess.TimeoutExpired:
        status = "timeout"
    finally:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait(timeout=2)
    if status == "timeout":
        out, err = child.communicate(timeout=2)
    return result | {"operation_result": status, "exit_code": child.returncode, "stderr": err.decode(errors="replace"), "stdout": out, "duration_seconds": round(time.monotonic() - start, 3)}


def profile(container):
    return "(version 1)\n(allow default)\n(deny file-read* (subpath " + json.dumps(str(container), ensure_ascii=True) + "))\n"


def commands(scratch, canary):
    scratch.mkdir()
    external = scratch / "external.py"
    external.write_text("import pathlib,sys\nsys.stdout.buffer.write(pathlib.Path(" + repr(str(canary)) + ").read_bytes())\n", encoding="ascii")
    runtime = scratch / "runtime.py"
    runtime.write_text("import json,pathlib,sys\nsettings=json.loads(pathlib.Path(sys.argv[1]).read_text())\nsys.stdout.buffer.write(pathlib.Path(settings['source']).read_bytes())\n", encoding="ascii")
    settings = scratch / "settings.json"
    settings.write_text(json.dumps({"source": str(canary)}), encoding="ascii")
    return {
        "direct_operand": ["/bin/cat", str(canary)],
        "inline_literal": [sys.executable, "-I", "-c", external.read_text(encoding="ascii")],
        "external_script": [sys.executable, "-I", str(external)],
        "runtime_config_path": [sys.executable, "-I", str(runtime), str(settings)],
    }


def observation(label, validation):
    if validation:
        return {field: "not_observed_synthetic" for field in ("tcc_prompt_appeared", "prompt_text", "new_app_data_entry_appeared", "responsible_application", "observation_notes")}
    print(f"Record UI observations for {label}; do not answer permission prompts or change settings.")
    answers = {}
    for field in ("tcc_prompt_appeared", "new_app_data_entry_appeared"):
        while (value := input(f"{field} [yes/no/unknown]: ").strip().lower()) not in ("yes", "no", "unknown"):
            pass
        answers[field] = value
    for field in ("prompt_text", "responsible_application", "observation_notes"):
        answers[field] = input(f"{field} (literal UI label/text, none, or unknown): ").strip() or "unknown"
    return answers


def build_writer(work, identifier, marker, record):
    contents = work / "CanaryWriter.app" / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    (contents / "Resources" / "canary.bin").write_bytes(marker)
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump({"CFBundleIdentifier": identifier, "CFBundleExecutable": "CanaryWriter", "CFBundleName": "Agent Guard Canary Writer", "CFBundlePackageType": "APPL", "CFBundleVersion": "1"}, file)
    entitlements = work / "entitlements.plist"
    with entitlements.open("wb") as file:
        plistlib.dump({"com.apple.security.app-sandbox": True}, file)
    source = work / "writer.m"
    source.write_text(WRITER, encoding="ascii")
    binary = contents / "MacOS" / "CanaryWriter"
    for command in (
        ["/usr/bin/clang", "-fobjc-arc", "-framework", "Foundation", str(source), "-o", str(binary)],
        ["/usr/bin/codesign", "--sign", "-", "--entitlements", str(entitlements), str(contents.parent)],
        ["/usr/bin/codesign", "--verify", "--strict", str(contents.parent)],
    ):
        result = run(command, deadline=60)
        record("fixture_build", result)
        if result["operation_result"] != "success":
            return None
    return binary


def os_probe(work, form, sandbox_exec, marker, record):
    """Check startup, an allowed read, and an EPERM/EACCES denied read outside Library."""
    denied = work / "probe-denied"
    denied.mkdir()
    target = denied / "canary.bin"
    target.write_bytes(marker)
    allowed = work / "probe-allowed.bin"
    allowed.write_bytes(marker)
    restricted = profile(denied)
    baseline = run(commands(work / "probe-baseline", target)[form])
    record("os_probe_baseline", baseline)
    if baseline["operation_result"] != "success" or baseline["stdout"] != marker:
        return "control_failed"
    if not sandbox_exec:
        return "unavailable"
    positive = run([sandbox_exec, "-p", restricted, *commands(work / "probe-positive", allowed)[form]])
    record("os_probe_allowed", positive)
    if positive["operation_result"] != "success" or positive["stdout"] != marker:
        return "unavailable"
    # A receipt distinguishes an executed denied open from launcher/compiler failure.
    code = "import errno,json,sys\ntry:\n open(sys.argv[1],'rb').read()\nexcept OSError as e:\n print(json.dumps({'denied_errno':e.errno}));sys.exit(13)\n"
    negative = run([sandbox_exec, "-p", restricted, sys.executable, "-I", "-c", code, str(target)])
    try:
        receipt = json.loads(negative["stdout"])
    except (ValueError, UnicodeError):
        receipt = {}
    record("os_probe_denied", negative, denied_open_receipt=receipt)
    if negative["exit_code"] == 13 and receipt.get("denied_errno") in (errno.EPERM, errno.EACCES):
        return "verified_on_synthetic_files"
    return "restriction_failed"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="new JSONL file in an existing directory outside the checkout and protected user folders")
    parser.add_argument("--guard-mode", required=True, choices=("on", "off"))
    parser.add_argument("--os-mode", required=True, choices=("on", "off"))
    parser.add_argument("--form", required=True, choices=FORMS)
    parser.add_argument("--guard", type=Path, help="assembled guard executable; required when --guard-mode is on")
    parser.add_argument("--synthetic-home", type=Path, help="new HOME outside real HOME; no native writer launch or TCC observations")
    args = parser.parse_args()
    validation = args.synthetic_home is not None
    real_home = Path.home().resolve()
    checkout = Path(__file__).resolve().parents[1]
    try:
        output = result_path(args.output or Path("/tmp") / ("agent-guard-read-enforcement-" + secrets.token_hex(8) + ".jsonl"), real_home, checkout)
    except ValueError as error:
        parser.error(str(error))
    if not output.parent.is_dir():
        parser.error("output needs an existing parent")
    home = args.synthetic_home.expanduser().resolve() if validation else real_home
    if validation and (home.is_relative_to(real_home) or home.exists()):
        parser.error("synthetic HOME must be a new directory outside the real HOME")
    if not validation and sys.platform != "darwin":
        parser.error("native cells require macOS")
    guard = args.guard.expanduser().resolve() if args.guard is not None else None
    if args.guard_mode == "on" and (guard is None or not guard.is_file()):
        parser.error("--guard must name an assembled guard executable when --guard-mode is on")
    work = output.with_name(output.name + ".artifacts")
    if output.exists() or work.exists():
        parser.error("output or artifact directory already exists")
    marker = ("SYNTHETIC_CANARY_" + secrets.token_hex(16) + "\n").encode("ascii")
    identifier = "com.example.agentguard.canary." + secrets.token_hex(8)
    container = home / "canaries" / identifier if validation else home / "Library" / "Containers" / identifier
    canary = container / "Data" / "canary.bin"
    configuration = f"guard_{args.guard_mode}__os_{args.os_mode}"
    with output.open("x", encoding="ascii") as evidence:
        def record(phase, result=None, *, observe=False, **extra):
            result = dict(result or {})
            stdout = result.pop("stdout", b"")
            row = {
                "configuration": configuration,
                "access_form": args.form,
                "phase": phase,
                "mode": "synthetic_validation" if validation else "native_observation",
                "time_unix": time.time(),
                "canary": str(canary),
                "canary_bytes_reached_stdout": marker in stdout,
                "canary_bytes": stdout.count(marker) * len(marker),
                "stdout_bytes": len(stdout),
                "tcc_prompt_appeared": "not_observed",
                "prompt_text": "not_observed",
                "new_app_data_entry_appeared": "not_observed",
                "responsible_application": "not_observed",
                **result,
                **extra,
            }
            if observe:
                row.update(observation(phase, validation))
            evidence.write(json.dumps(row, ensure_ascii=True) + "\n")
            evidence.flush()

        try:
            work.mkdir()
            record("artifacts_created", created=[str(output), str(work)], fixture_identifier=identifier, canary_sha256=hashlib.sha256(marker).hexdigest(), expected_canary_bytes=len(marker), macos_kernel=os.uname().release, python=sys.version.split()[0])
            restriction = os_probe(work, args.form, shutil.which("sandbox-exec"), marker, record) if args.os_mode == "on" else "not_requested"
            record("os_restriction_status", restriction_status=restriction)
            if args.os_mode == "on" and restriction != "verified_on_synthetic_files":
                record("operation", operation_result="not_run_os_restriction_unavailable", exit_code=None)
                return 3
            if validation:
                home.mkdir(parents=True)
                record("baseline", observe=True)
            else:
                print("Before setup: inspect Files & Folders and Full Disk Access without changing them.")
                record("baseline", observe=True, prior_app_data_entry=input("Existing App Data entry for the launching application [yes/no/unknown]: "), prior_full_disk_access=input("Existing Full Disk Access [yes/no/unknown]: "), launch_application=input("Terminal application and launch arrangement: "), clean_state_basis=input("Disposable account/snapshot identifier, or unknown: "))
            binary = build_writer(work, identifier, marker, record)
            if binary is None:
                record("setup", operation_result="fixture_build_failed", observe=True)
                return 4
            if validation:
                canary.parent.mkdir(parents=True)
                canary.write_bytes(marker)
                setup = {"operation_result": "synthetic_fixture_created", "exit_code": 0, "stdout": b""}
            else:
                print("Starting the canary owner; watch for setup prompts and entries.", flush=True)
                setup = run([str(binary), str(canary.parent)])
                try:
                    receipt = json.loads(setup["stdout"])
                except (ValueError, UnicodeError):
                    receipt = {}
                if setup["operation_result"] != "success" or receipt != {"target": str(canary), "bytes_written": len(marker)}:
                    record("setup", setup, observe=True, fixture_receipt=receipt, fixture_status="unverified")
                    return 4
            record("setup", setup, observe=True, created=[str(container)], fixture_status="synthetic_only" if validation else "owner_write_receipt")
            command = commands(work / "access", canary)[args.form]
            env = dict(os.environ, HOME=str(home)) if validation else None
            if args.guard_mode == "on":
                event = {"tool_name": "Bash", "tool_input": {"command": shlex.join(command)}, "cwd": str(work)}
                guard_command = [str(guard), "--runtime", "claude"]
                if args.os_mode == "on":
                    guard_command = [shutil.which("sandbox-exec"), "-p", profile(container), *guard_command]
                print("Running guard preflight; watch UI separately.", flush=True)
                checked = run(guard_command, env=env, stdin=json.dumps(event).encode("ascii"))
                decision = "allow" if checked["operation_result"] == "success" else "deny" if checked["exit_code"] == 2 else "failure"
                record("guard", checked, observe=True, guard_decision=decision, checked_command=shlex.join(command))
                if decision != "allow":
                    record("operation", operation_result="not_run_guard_" + decision, exit_code=None, intended_command=shlex.join(command), observe=True)
                    return 0
            actual = [shutil.which("sandbox-exec"), "-p", profile(container), *command] if args.os_mode == "on" else command
            print("Running access; watch prompts now. Leave them unanswered; the deadline terminates the request.", flush=True)
            record("operation", run(actual, env=env), observe=True, restriction_status=restriction)
            return 0
        except (OSError, EOFError, KeyboardInterrupt) as error:
            record("experiment_failure", operation_result="experiment_failed", error=str(error))
            return 4
        finally:
            record("retained_artifacts", paths=[str(work), str(container)], note="No permission configuration was changed. Retention is not evidence of a TCC entry; failed setup may leave partial artifacts.")


if __name__ == "__main__":
    raise SystemExit(main())
