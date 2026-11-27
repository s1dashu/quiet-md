# Installing Quiet.md for an AI agent

These instructions are for an AI coding agent (Claude Code, Codex or another)
that a user asked to "install the Quiet.md skill and CLI". Follow the steps in
order and use the section for the agent you are. Ask the user before changing
their shell profile.

Quiet.md is a local-first notes app. Its documents are files on the user's
computer, and the `quiet` command reads and writes them. The `quiet-md` skill
teaches you to use that command. Install both: the skill does nothing without
the command.

## 1. Find or install the `quiet` command

Check first:

```sh
quiet --version
```

If it prints `quiet <version>`, go to step 2. Otherwise the command ships
inside the Quiet.md app:

- **macOS:** the app is at `/Applications/quiet.md.app`. Ask the user to open
  Quiet.md and click **Settings › Agent Connector › Install Command**
  (设置 › 连接 Agent › 安装命令行工具), or link it yourself (no administrator password needed):

  ```sh
  mkdir -p ~/.local/bin
  ln -sf /Applications/quiet.md.app/Contents/MacOS/quiet ~/.local/bin/quiet
  ```

  If `~/.local/bin` is not on `PATH`, ask the user before adding
  `export PATH="$HOME/.local/bin:$PATH"` to `~/.zprofile` (or their shell's
  profile), and use `~/.local/bin/quiet` in the meantime.
- **Windows:** the installer puts `quiet` on the user's `PATH`; open a new
  terminal. It is also at `%LOCALAPPDATA%\Programs\quiet.md\quiet.exe`.
- **Linux:** `quiet` is in the package's `bin/` folder, beside `quiet_md`.
  Link it into a folder on `PATH`, e.g. `ln -sf <package>/bin/quiet ~/.local/bin/quiet`.

If Quiet.md is not installed, its app has no `quiet` inside, or `quiet
--version` is below 0.2.0 (versions without 连接 Agent / Agent Connector in
Settings, which also lack commands the skill uses), tell the user to update to
the latest Quiet.md from https://quiet.md and open it once, then continue.

## 2. Install the skill

The skill lives in this repository at `skills/quiet-md/`. Install it with the
[skills](https://github.com/vercel-labs/skills) CLI (needs Node.js 22.20 or
later). The flags below install for the user (`-g`) without questions (`-y`);
without `-y` it stops to ask, and without `-g` it installs into the current
folder only. Cloning the repository takes up to a minute or two.

### Claude Code

```sh
npx skills add s1dashu/quiet-md -g -a claude-code -y
```

This puts the skill in `~/.claude/skills/quiet-md/`.

### Codex

```sh
npx skills add s1dashu/quiet-md -g -a codex -y
```

This puts the skill in `~/.agents/skills/quiet-md/`. (Codex's own skill
installer, putting it in `~/.codex/skills/quiet-md/`, works too, but
`npx skills update` then leaves it out.)

### Other agents

```sh
npx skills add s1dashu/quiet-md -g -y
```

Add `-a <agent>` for your agent if the skills CLI knows it
(`npx skills add --help` lists them). Without Node.js, copy the folder
`skills/quiet-md/` from this repository into your agent's skills folder.

## 3. Check

```sh
quiet --version
quiet ls
```

`quiet ls` lists the folders Quiet.md has open. If it says there are none,
the user can name the folder their notes are in and `quiet folders add PATH`
puts it in Quiet.md's sidebar. Then tell the user it is installed, and that a
new session picks up the skill.
