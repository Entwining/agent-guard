package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

func TestProfileIdentityAndDenial(t *testing.T) {
	for _, identity := range []struct{ kind, value string }{
		{"bundleID", "com.example.canary"},
		{"path", `/Applications/Canary & "Test"/client`},
	} {
		t.Run(identity.kind, func(t *testing.T) {
			root := t.TempDir()
			t.Setenv("HOME", root)
			t.Setenv("TMPDIR", root)
			output := filepath.Join(root, "appdata.mobileconfig")
			requirement := `identifier "com.example.canary" and anchor apple`
			args := []string{identity.kind, identity.value, requirement, output}
			var printed bytes.Buffer
			if err := generate(args, &printed); err != nil {
				t.Fatal(err)
			}
			body, err := exec.Command("/usr/bin/plutil", "-convert", "json", "-o", "-", output).Output()
			if err != nil {
				t.Fatal(err)
			}
			var profile struct {
				PayloadType, PayloadScope, PayloadUUID, PayloadIdentifier string
				PayloadContent                                            []struct {
					PayloadType, PayloadUUID, PayloadIdentifier string
					Services                                    map[string][]struct {
						Identifier, IdentifierType, CodeRequirement string
						Allowed                                     *bool
					}
				}
			}
			if err := json.Unmarshal(body, &profile); err != nil {
				t.Fatal(err)
			}
			if profile.PayloadType != "Configuration" || profile.PayloadScope != "System" || len(profile.PayloadContent) != 1 {
				t.Fatalf("wrong device profile: %s", body)
			}
			payload := profile.PayloadContent[0]
			rows := payload.Services["SystemPolicyAppData"]
			if payload.PayloadType != "com.apple.TCC.configuration-profile-policy" || len(payload.Services) != 1 || len(rows) != 1 {
				t.Fatalf("wrong PPPC service: %s", body)
			}
			row := rows[0]
			if row.Identifier != identity.value || row.IdentifierType != identity.kind || row.CodeRequirement != requirement || row.Allowed == nil || *row.Allowed {
				t.Fatalf("identity or denial changed: %s", body)
			}
			uuid := regexp.MustCompile(`(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)
			if !uuid.MatchString(profile.PayloadUUID) || !uuid.MatchString(payload.PayloadUUID) || profile.PayloadUUID == payload.PayloadUUID || payload.PayloadIdentifier != profile.PayloadIdentifier+".pppc" {
				t.Fatalf("payload identities collide: %s", body)
			}
			info, err := os.Stat(output)
			if err != nil || info.Mode().Perm() != 0600 {
				t.Fatalf("profile mode: %v, %v", info, err)
			}
			for _, text := range []string{"Plist validation: PASS", "Runtime enforcement: unverified", "A supervised Mac running macOS 14 or newer, admin rights", "baseline must open successfully", "Manual profile installation", "Allowed=false"} {
				if !strings.Contains(printed.String(), text) {
					t.Fatalf("missing limitation or inspection step: %s", text)
				}
			}
			before, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			if err := generate(args, &printed); err == nil {
				t.Fatal("existing profile overwritten")
			}
			after, err := os.ReadFile(output)
			if err != nil || !bytes.Equal(before, after) {
				t.Fatal("existing profile changed")
			}
		})
	}
}

func TestInvalidProfilesWriteNothing(t *testing.T) {
	root := t.TempDir()
	t.Setenv("HOME", root)
	t.Setenv("TMPDIR", root)
	t.Chdir(root)
	output := filepath.Join(root, "invalid.mobileconfig")
	for _, args := range [][]string{
		{"unknown", "com.example.canary", "anchor apple", output},
		{"path", "relative", "anchor apple", output},
		{"bundleID", "com.example.canary", "not a requirement", output},
		{"bundleID", "com.example.canary", `designated => identifier "com.example.canary"`, output},
		{"bundleID", "com.example.canary", "anchor apple\n", output},
		{"bundleID", "com.example.canary\n", "anchor apple", output},
		{"bundleID", "invalid&identifier", "anchor apple", output},
		{"bundleID", "com.example.canary", "anchor apple", "relative.mobileconfig"},
	} {
		if err := generate(args, &bytes.Buffer{}); err == nil {
			t.Fatalf("invalid profile accepted: %q", args)
		}
		if _, err := os.Lstat(args[3]); !os.IsNotExist(err) {
			t.Fatalf("invalid input wrote output: %v", err)
		}
	}
}

func TestProfileCheckoutAndExistingSymlink(t *testing.T) {
	root := t.TempDir()
	t.Setenv("HOME", root)
	checkout := filepath.Join(root, "checkout")
	if err := os.Mkdir(checkout, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(checkout, ".git"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "alias")
	if err := os.Symlink(checkout, alias); err != nil {
		t.Fatal(err)
	}
	for _, directory := range []string{checkout, alias} {
		args := []string{"bundleID", "com.example.canary", "anchor apple", filepath.Join(directory, "profile.mobileconfig")}
		if err := generate(args, &bytes.Buffer{}); err == nil || !strings.Contains(err.Error(), "outside every checkout") {
			t.Fatalf("checkout output accepted: %v", err)
		}
	}
	target := filepath.Join(root, "preserved")
	if err := os.WriteFile(target, []byte("preserved"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "profile.mobileconfig")
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
	if err := generate([]string{"bundleID", "com.example.canary", "anchor apple", link}, &bytes.Buffer{}); err == nil {
		t.Fatal("existing output symlink followed")
	}
	body, err := os.ReadFile(target)
	if err != nil || string(body) != "preserved" {
		t.Fatal("symlink target changed")
	}
}

func TestInstructionsNeedNoSelectedClient(t *testing.T) {
	var output bytes.Buffer
	if err := generate([]string{"--instructions"}, &output); err != nil {
		t.Fatal(err)
	}
	for _, text := range []string{"/usr/bin/profiles status -type enrollment", "baseline must open successfully", "agent-guard-profile", "enforcement remain unverified"} {
		if !strings.Contains(output.String(), text) {
			t.Fatalf("missing instruction: %s", text)
		}
	}
}

func TestWrittenProfileValidationFailure(t *testing.T) {
	root := t.TempDir()
	t.Setenv("HOME", root)
	output := filepath.Join(root, "invalid.mobileconfig")
	if err := writeProfile(output, "not a plist"); err == nil || !strings.Contains(err.Error(), "profile written but validation failed") {
		t.Fatalf("written validation failure was hidden: %v", err)
	}
	body, err := os.ReadFile(output)
	if err != nil || string(body) != "not a plist" {
		t.Fatalf("failed artifact was lost: %q, %v", body, err)
	}
}
