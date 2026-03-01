## Tool Usage Preferences
- **NEVER** use heredocs (`<<EOF`) or `echo` in the terminal to create or edit files.
- **ALWAYS** use the `createFile` or `editFile` (replace_string) tools for any file modifications.
- Use the terminal **ONLY** for running commands (e.g., `npm install`, `ls`, `git status`) or executing existing scripts.

## Post-Edit Workflow
- **Mandatory Final Step:** After completing all code edits and verifying they work, you MUST commit the changes to the current branch.
- **Commit Procedure:**
  1. Stage all changes: `git add .`
  2. Generate a concise, imperative-style commit message (e.g., "feat: update nixium styles").
  3. Execute the commit: `git commit -m "<your message>"`
  4. Push the commit to the remote repository: `git push`