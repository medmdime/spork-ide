# M10 — Credentials, askpass, SSH keys, signing

Fork surfaces: AskPassController, SshPassphraseController, ConfigureSshKeysController, GenerateSshKeyController, helper `fork_askpasswd`.
Phase: P2 (spike in week 1 on Windows). Components: `Dialog`, `Input` (masked), `Checkbox`, `List`, `Button`.

## 1. Order of credential sources (HTTPS)
1. The user's configured git credential helper (osxkeychain, Git Credential Manager, libsecret…) runs first — we don't override `credential.helper`.
2. If the remote matches a Spork **account** (M15) → `spork-askpass` answers username/token from the keyring without UI.
3. Otherwise `spork-askpass` asks the app → **AskPass dialog**: prompt text from git, Password field, "Remember" checkbox.
   Remember → after the op succeeds: `git credential approve` with protocol/host/path/username/password (so the user's own helper stores it); on auth failure → `git credential reject`.

## 2. `spork-askpass` protocol
- Invoked by git as `spork-askpass "<prompt>"`, by ssh as `SSH_ASKPASS` with `SSH_ASKPASS_REQUIRE=force` (also set `DISPLAY=:0` on Linux if unset, required by older OpenSSH).
- Connects to `$SPORK_IPC` (a localhost TCP port opened by the app, guarded by a random per-launch token in `$SPORK_IPC_TOKEN`; std-only and identical on every OS), sends
  `{"v":1,"kind":"askpass","job":"<job id>","prompt":"Password for 'https://u@github.com': ","repo":"/path"}`, receives `{"answer":"…"}` or `{"cancel":true}`.
- Prints the answer + `\n` to stdout, exit 0; cancel → exit 1. Timeout 5 min. Never logs the answer.
- Prompt classification: `Username for` / `Password for` / `Enter passphrase for key '<path>'` / `Are you sure you want to continue connecting (yes/no` (host key → show fingerprint dialog, answer `yes`/`no`) / `Verification code` (2FA).

## 3. SSH passphrases
Passphrase dialog shows key path; "Remember" → add to agent: macOS `ssh-add --apple-use-keychain <key>`, Linux/Windows `ssh-add <key>` (agent must run; on Windows check `ssh-agent` service and explain how to enable it).
Windows: prefer OpenSSH from MinGit or system (`core.sshCommand` respected). PuTTY/plink users: respected via `GIT_SSH`.

## 4. Configure SSH keys
List `~/.ssh/*.pub` (+ `IdentityFile` entries from `~/.ssh/config`), show public key with Copy button, Open existing key…
Generate: file name (default `id_ed25519`), email comment, passphrase (optional), type **ed25519** (default; RSA 4096 option) via `ssh-key` crate, permissions 0600; then add to agent; "Copy public key" + "Open GitHub/GitLab SSH settings page" for linked accounts.

## 5. Commit signing
GPG: let `gpg` + pinentry handle passphrases (pinentry-mac / pinentry-gnome / Gpg4win); we just pass `-S`. Detect missing pinentry → explain. SSH signing (`gpg.format=ssh`) supported through repo settings. Show signature status in details: `git log --format=%G?%x00%GS%x00%GK`.

## Security rules
Tokens only in OS keyring; never in config files, logs, URLs or crash reports. IPC socket permissions verified at startup. Redact `Authorization`, `password`, URL userinfo in all job logs.

## Tests
Fake git (shell script calling `$GIT_ASKPASS`) → assert dialog shown + answer returned; cancel path; Windows named-pipe ACL test; credential approve/reject called correctly.

## Open questions
1. How `fork_askpasswd` talks to Fork (socket? XPC? file?) — ask REA to describe the transport only. 2. When Fork stores credentials in its own keychain items vs git's helper. 3. Host-key prompt handling.
