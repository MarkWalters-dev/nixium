## Tool Usage Preferences
- **NEVER** use heredocs (`<<EOF`) or `echo` in the terminal to create or edit files.
- **ALWAYS** use the `createFile` or `editFile` (replace_string) tools for any file modifications.
- Use the terminal **ONLY** for running commands (e.g., `npm install`, `ls`, `git status`) or executing existing scripts.

## Post-Edit Workflow

### 1. Hardware Constraints (Chromebook vs. P72)
- **Local Environment:** Low-power Chromebook. **NEVER** run `cargo build`, `npm build`, or heavy compilation/testing locally.
- **Verification Rule:** If a build or test is required, you MUST execute it on the P72 server.

### 2. Mandatory Push-to-Build Workflow
Once edits are finished, follow this exact sequence without asking for permission:

**Phase 1: Local Commit & Push (Chromebook)**
1. Stage all changes: `git add .`
2. Generate a concise, imperative-style commit message (e.g., "feat: update nixium styles").
3. Execute: `git commit -m "[Your Generated Message]"`
4. Execute: `git push`

**Phase 2: Remote Build & Verify (P72)**
*Only if verification is needed:*
1. SSH to the build server: `ssh p72`
2. Navigate to project: `cd ~/code/nixium`
3. Update the remote repo: `git pull`
4. Execute build: `cargo build` (or `npm run build` for Svelte/Vite)
5. Exit SSH once the build status is confirmed.



