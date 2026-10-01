package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

const instructions = `A supervised Mac running macOS 14 or newer, admin rights, and access to its
user-approved MDM service are required. Local admin rights alone are insufficient.
No enrollment, policy installation, or TCC reset is performed by this generator.

1. Check route availability in a plain terminal:
   /usr/bin/sw_vers -productVersion
   /usr/bin/profiles status -type enrollment
   /usr/bin/id -Gn
   Confirm supervision and user-approved device enrollment in the MDM inventory
   or System Settings > General > Device Management. Admin group membership is
   only a local rights check; MDM deployment permission must be confirmed there.
   If supervision or MDM access is absent, this route is unavailable.

2. Establish the responsible client before selecting an identity:
   /usr/bin/log stream --debug --predicate 'subsystem == "com.apple.TCC" AND eventMessage BEGINSWITH "AttributionChain"'
   In a separate session, use the intended agent launch path to read a harmless
   canary created by its owning app in that app's actual container. Do not scan
   containers or use existing personal files. Record the responsible identity;
   do not assume it is the shell, hook, or child. If attribution is unavailable,
   the identity and enforcement remain unverified. Stop rather than guess.
   For the explicitly selected app or binary (substitute its actual path):
   /usr/bin/codesign -dv --verbose=4 '/absolute/path/to/selected-client'
   /usr/bin/codesign -dr - '/absolute/path/to/selected-client'
   Use bundleID for an app bundle, path for a nonbundled binary. Copy only the
   designated requirement after "designated =>", not that prefix. This metadata
   is public; do not inspect credential stores or TCC databases.

3. Record a successful baseline before deploying any profile:
   Have the owning app create a non-sensitive canary in its actual container.
   Choose the selected client's ordinary read facility and its normal launch path.
   For a shell client, substitute that one file in this command; no directory listing:
   canary='/absolute/path/to/owner-app-created-canary'
   /bin/dd if="$canary" of=/dev/null bs=1 count=1
   The baseline must open successfully. Record the exit and TCC attribution.
   If a hook blocks the read, stop; that is not a TCC result. Do not retry through
   another tool, path, or spelling, or change hooks or permissions. A Terminal-only
   run cannot establish another app's policy. Synthetic HOME lookalikes cannot
   establish real App Data enforcement.

4. Generate the unsigned profile with four explicitly selected arguments:
   go run ./cmd/agent-guard-profile 'bundleID-or-path' 'selected-identifier' 'designated-code-requirement' '/absolute/output/outside-checkouts/appdata-deny.mobileconfig'
   The generator compiles the requirement and checks plist syntax locally.
   This deterministic validation does not prove client matching or TCC denial.
   Have the MDM administrator review the selected identity and upload the file
   as a custom macOS device-channel profile, then scope it to this test Mac.
   Wait for the MDM installation acknowledgment. Manual profile installation
   is unsupported for PPPC; profiles cannot install profiles on modern macOS.

5. Inspect the delivered device profile locally (do not share unrelated data):
   sudo /usr/bin/profiles show -type configuration
   Match the printed PayloadIdentifier, selected identity, CodeRequirement,
   Services.SystemPolicyAppData and Allowed=false with the generated profile.

6. After the client exits fully and is relaunched, repeat the exact baseline read:
   The managed run must fail to open with an OS permission error, and TCC
   attribution must identify the selected client. A hook denial is not an OS
   result; stop it without a retry. Repeat an ordinary project-file read as an
   allow control. Record macOS version,
   profile acknowledgment, identity, exit codes, and OS errors; if any evidence
   is missing, report runtime enforcement as unverified.
   For recovery, have the MDM administrator remove only this test profile from
   this Mac and confirm removal, then relaunch the client and repeat the baseline.
`

func main() {
	if err := generate(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: %s\n", err)
		os.Exit(1)
	}
}

func generate(args []string, stdout io.Writer) error {
	if len(args) == 1 && args[0] == "--instructions" {
		_, err := fmt.Fprint(stdout, instructions)
		return err
	}
	if len(args) != 4 {
		return errors.New("expected identifier type, identifier, designated requirement, and absolute output path; use --instructions for exact steps")
	}
	typeName, identifier, requirement, output := args[0], args[1], args[2], args[3]
	if typeName != "bundleID" && typeName != "path" {
		return errors.New("IdentifierType must be bundleID or path")
	}
	for _, text := range []string{identifier, requirement} {
		if strings.TrimSpace(text) == "" || strings.ContainsFunc(text, func(r rune) bool { return r < 32 }) {
			return errors.New("identity and requirement must be nonempty text without control characters")
		}
	}
	if typeName == "path" && !filepath.IsAbs(identifier) {
		return errors.New("a binary identifier must be an absolute installation path")
	}
	if typeName == "bundleID" && !regexp.MustCompile(`^[A-Za-z0-9.-]+$`).MatchString(identifier) {
		return errors.New("invalid bundle ID")
	}
	if strings.Contains(requirement, "designated =>") {
		return errors.New(`supply only the requirement after "designated =>"`)
	}
	if !filepath.IsAbs(output) {
		return errors.New("output must be an absolute path outside checkouts")
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(output))
	if err != nil {
		return err
	}
	for directory := parent; ; directory = filepath.Dir(directory) {
		if _, err := os.Lstat(filepath.Join(directory, ".git")); err == nil {
			return errors.New("output must be outside every checkout")
		} else if !errors.Is(err, os.ErrNotExist) {
			return err
		}
		if directory == filepath.Dir(directory) {
			break
		}
	}
	if err := validate("/usr/bin/csreq", "-r", "="+requirement, "-t"); err != nil {
		return fmt.Errorf("invalid code requirement: %w", err)
	}
	uuid, err := randomUUID()
	if err != nil {
		return err
	}
	payloadUUID, err := randomUUID()
	if err != nil {
		return err
	}
	payloadIdentifier := "com.loophubs.agent-guard.appdata-deny." + uuid
	var identity, code bytes.Buffer
	if err := xml.EscapeText(&identity, []byte(identifier)); err != nil {
		return err
	}
	if err := xml.EscapeText(&code, []byte(requirement)); err != nil {
		return err
	}
	profile := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>PayloadType</key><string>Configuration</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadScope</key><string>System</string>
<key>PayloadUUID</key><string>%s</string>
<key>PayloadIdentifier</key><string>%s</string>
<key>PayloadDisplayName</key><string>Agent Guard App Data Denial</string>
<key>PayloadContent</key><array><dict>
<key>PayloadType</key><string>com.apple.TCC.configuration-profile-policy</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadUUID</key><string>%s</string>
<key>PayloadIdentifier</key><string>%s.pppc</string>
<key>PayloadDisplayName</key><string>Selected Client App Data Denial</string>
<key>Services</key><dict><key>SystemPolicyAppData</key><array><dict>
<key>Identifier</key><string>%s</string>
<key>IdentifierType</key><string>%s</string>
<key>CodeRequirement</key><string>%s</string>
<key>Allowed</key><false/>
</dict></array></dict>
</dict></array></dict></plist>
`, uuid, payloadIdentifier, payloadUUID, payloadIdentifier, identity.String(), typeName, code.String())
	if err := writeProfile(output, profile); err != nil {
		return err
	}
	fmt.Fprintf(stdout, "Unsigned profile: %s\nPayloadIdentifier: %s\nSelected client: %s %s\nPlist validation: PASS\nRuntime enforcement: unverified\n", output, payloadIdentifier, typeName, identifier)
	fmt.Fprintf(stdout, "Inspect this artifact: /usr/bin/plutil -p %s\nInspect its exact denial: /usr/bin/plutil -extract PayloadContent.0.Services.SystemPolicyAppData.0.Allowed raw -o - %s\n", quote(output), quote(output))
	_, err = fmt.Fprint(stdout, instructions)
	return err
}

func writeProfile(output, profile string) error {
	file, err := os.OpenFile(output, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	_, err = io.WriteString(file, profile)
	closed := file.Close()
	if err != nil {
		return err
	}
	if closed != nil {
		return closed
	}
	if err := validate("/usr/bin/plutil", "-lint", output); err != nil {
		return fmt.Errorf("profile written but validation failed: %w", err)
	}
	return nil
}

func validate(path string, args ...string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, path, args...)
	cmd.WaitDelay = 250 * time.Millisecond
	output, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("%w: %s", err, strings.TrimSpace(string(output)))
	}
	return nil
}

func randomUUID() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	b[6], b[8] = b[6]&0x0f|0x40, b[8]&0x3f|0x80
	return fmt.Sprintf("%X-%X-%X-%X-%X", b[:4], b[4:6], b[6:8], b[8:10], b[10:]), nil
}

func quote(value string) string { return "'" + strings.ReplaceAll(value, "'", "'\\''") + "'" }
