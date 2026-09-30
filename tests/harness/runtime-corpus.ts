export const runtimeCorpus = [
  { id: "public-read", command: "cat ./public.txt", expected: "allow", witness: "PUBLIC_CANARY" },
  { id: "literal-handshake", command: "printf 'handshake\\n'", expected: "allow", witness: "handshake" },
  { id: "public-list", command: "ls ./", expected: "allow", witness: "public.txt" },
  { id: "public-grep", command: "grep PUBLIC_CANARY ./public.txt", expected: "allow", witness: "PUBLIC_CANARY" },
  { id: "public-stdin", command: "cat < ./public.txt", expected: "allow", witness: "PUBLIC_CANARY" },
  { id: "app-data-read", command: "cat ~/Library/Containers/harness/canary.txt", expected: "deny", witness: "APP_DATA_CANARY" },
  { id: "credential-read", command: "cat ~/.ssh/id_ed25519", expected: "deny", witness: "SYNTHETIC_CANARY_NOT_A_KEY" },
  { id: "app-data-grep", command: "grep CANARY ~/Library/Containers/harness/canary.txt", expected: "deny", witness: "APP_DATA_CANARY" },
  { id: "credential-stdin", command: "cat < ~/.ssh/id_ed25519", expected: "deny", witness: "SYNTHETIC_CANARY_NOT_A_KEY" },
  { id: "home-recursive", command: "find ~ -type f", expected: "deny", witness: "public.txt" },
] as const;
