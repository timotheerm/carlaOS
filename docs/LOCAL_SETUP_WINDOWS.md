# Local development setup — Windows 11

This is the recommended native Windows setup for developing CarlaOS. The repository is a Rust workspace with a Tauri 2 + React + TypeScript shell, so use PowerShell or Windows Terminal on Windows rather than setting up WSL unless you specifically need Linux tooling.

## 1. Install the tools

Open **PowerShell**. Install the command-line tools with WinGet:

```powershell
winget install --id Git.Git -e
winget install --id GitHub.cli -e
winget install --id OpenJS.NodeJS.22 -e
winget install --id Rustlang.Rustup -e
```

VS Code is optional but recommended:

```powershell
winget install --id Microsoft.VisualStudioCode -e
```

Install Claude Code using Anthropic's recommended native Windows installer:

```powershell
irm https://claude.ai/install.ps1 | iex
```

Install **Visual Studio Build Tools** from [Microsoft](https://visualstudio.microsoft.com/visual-cpp-build-tools/). In the installer select **Desktop development with C++** and keep the MSVC compiler and Windows 10/11 SDK selected. The full Visual Studio IDE is not required.

Windows 11 normally includes the Microsoft Edge WebView2 runtime required by Tauri. Check the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/) if the runtime or native build dependencies are missing.

Install [Obsidian](https://obsidian.md/download) only if you want a visual interface for the Markdown knowledge base. It is optional; the notes remain ordinary Markdown files in Git.

Close and reopen the terminal after installations so PATH changes are loaded.

## 2. Verify the installation

```powershell
git --version
gh --version
node --version
npm --version
rustc --version
cargo --version
rustup show
claude --version
claude doctor
```

Use Node.js 22.x to match the repository's CI setup. The Rust default should be the stable MSVC toolchain:

```powershell
rustup default stable-msvc
```

Sign in to GitHub CLI:

```powershell
gh auth login
gh auth status
```

Choose GitHub.com, HTTPS, and browser authentication. This lets local Claude Code sessions inspect PRs, checks and branches through `gh`. Do not paste access tokens into prompts or commit them.

## 3. Clone CarlaOS

Keep the main clone in a stable development folder. For example:

```powershell
New-Item -ItemType Directory -Force D:\Development | Out-Null
Set-Location D:\Development
git clone https://github.com/timotheerm/carlaOS.git
Set-Location .\carlaOS
git status --short --branch
```

Open the repository root in VS Code:

```powershell
code .
```

Open Obsidian, choose **Open folder as vault**, and select the repository root. Start with `knowledge/START-HERE.md`. Do not create a second, disconnected copy of the knowledge base.

## 4. Install dependencies and run checks

From the repository root:

```powershell
cargo test --locked
```

Install the frontend dependencies from the checked-in lockfile:

```powershell
Set-Location .\apps\shell
npm ci
npm run build
```

Run the Tauri desktop development app from `apps/shell`:

```powershell
npx tauri dev
```

The first Rust build takes longer because Cargo downloads and compiles dependencies. Keep the terminal open while Tauri is running.

For a full pre-PR check, return to the repository root and run:

```powershell
Set-Location D:\Development\carlaOS
cargo test --locked
scripts\check-windows.sh
```

The shell script is written for Git Bash. In PowerShell, run it through Git Bash:

```powershell
& 'C:\Program Files\Git\bin\bash.exe' scripts/check-windows.sh
```

Then check the frontend again:

```powershell
Set-Location .\apps\shell
npm run build
```

A local build is not the same as validating a real Ally. Hardware-specific behaviour still needs a real ASUS ROG Xbox Ally test.

## 5. Start Claude Code correctly

Start Claude Code from the **repository root**, not `apps/shell`, so it loads the project's `CLAUDE.md`, skills and agents:

```powershell
Set-Location D:\Development\carlaOS
claude
```

Useful commands inside Claude Code:

- `/start-task` — inspect repository state, open PR dependencies and establish a safe branch before coding.
- `/agents` — inspect and select the project agents.
- `/review-pr` — review a PR systematically.
- `/finish-task` — update durable project memory after verified work.
- `/compact` — compress a long session when needed.
- `/doctor` — diagnose the Claude Code environment.

For unfamiliar or architectural work, start in plan mode. Approve the plan before letting the main session edit files. Leave permissions on their safe defaults; do not grant blanket approval for destructive shell or Git commands.

## 6. Use branches and worktrees

Start ordinary work from the latest main:

```powershell
git fetch origin --prune
git switch main
git pull --ff-only origin main
git switch -c feat/short-task-name
```

Use one PR per coherent feature, not one PR per agent. Before pushing:

```powershell
git status --short
git diff --check
git diff
```

For a genuinely independent second task, create a separate worktree:

```powershell
git fetch origin --prune
New-Item -ItemType Directory -Force D:\Development\CarlaOS-worktrees | Out-Null
git worktree add D:\Development\CarlaOS-worktrees\settings -b feat/settings origin/main
code D:\Development\CarlaOS-worktrees\settings
```

Start a separate Claude Code session in that worktree. Do not have two writing sessions modify the same working directory or branch. Dependent work belongs on the parent feature branch or in a deliberate stacked PR; a worktree does not remove code dependencies.

## 7. Standard PR loop

```powershell
git push -u origin HEAD
gh pr create --base main
gh pr checks
gh pr view --web
```

After review and CI, merge with the repository's normal squash strategy. Do not merge merely because a PR is marked mergeable: inspect the diff, dependencies and every required check. A merge to `main` can trigger the project's automated beta pipeline; read [RELEASES.md](RELEASES.md) before release-related changes.

## Recommended editor extensions

Start lean:
- Rust Analyzer
- TypeScript and JavaScript Language Features (built into VS Code)
- ESLint only if/when the project adds ESLint
- GitLens is optional; Git and `gh` already cover the core workflow

Avoid installing a large collection of AI plugins. Claude Code is the primary coding interface; use Obsidian as a viewer/editor for project memory and a second model as an independent reviewer.
