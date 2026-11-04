#!/usr/bin/env python3
"""Folder documents, end to end, on temporary folders and repositories only.

usage: python3 tools/harness/folder_smoke.py [scenario ...]
  FDL_BIN  binary to test (default target/release/quiet_md)
  Screenshots and logs stay in the printed work directory.
Every scenario drives the real app and asserts on file bytes and app state.
"""
import os, re, shutil, subprocess, sys, tempfile, time
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
binary = os.environ.get("FDL_BIN") or str(repo / "target/release/quiet_md")
work = Path(tempfile.mkdtemp(prefix="quiet-folder-")).resolve()
GIT = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.invalid", "PATH": "/opt/homebrew/bin:/usr/bin:/bin", "HOME": os.path.expanduser("~")}
failures = []

MESSY = ("---\ntitle: \"Doc title\"\ntags:\n  - one\n  - two\n---\n\n# Heading __kept__ #\n\nFirst paragraph with __underscored bold__ and *star*,\n"
         "a hard wrapped line\\_with escape &amp; entity.\n\n* [ ] star task\n* [X] done task\n   1) nested odd indent\n\n<!-- a comment stays -->\n\n"
         "```mermaid\nflowchart LR\n  A[Start] --> B[Finish]\n```\n\n| a | b |\n|:--|--:|\n| 1 | *2* |\n\n[link text][ref] and [other](sub/other.md#target-here).\n\n"
         "![pixel](pixel.png)\n\n***\n\n[ref]: https://example.com \"Title\"\n\n\n")

def git(cwd, *args): subprocess.run(["git", *args], cwd=cwd, env=GIT, check=True, capture_output=True)
def folder(name, files):
    root = work / name / "folder"; root.mkdir(parents=True)
    for rel, text in files.items():
        p = root / rel; p.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(text, bytes): p.write_bytes(text)
        else: p.write_bytes(text.encode())
    return root
def run(name, root, script, lib=None, expect_crash=False):
    w = work / name; w.mkdir(exist_ok=True)
    lib = lib or w / "lib"
    if not lib.exists(): shutil.copytree(repo / "tools/harness/lib_clean", lib)
    (w / "s.txt").write_text(f"move 5 5\nresize 1280 800\nwait 300\npick {root}\nkey cmd-o\nwait 1500\n" + script + ("" if expect_crash else "\nquit\n"))
    env = {"HOME": os.path.expanduser("~"), "QUIET_MD_HOME": str(lib), "FDL_SCRIPT": str(w / "s.txt"), "FDL_SHOTS": str(w), "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "FDL_NOBLINK": "1"}
    log = w / "log"
    with open(log, "wb") as out: proc = subprocess.run([binary], env=env, cwd=w, stdout=out, stderr=subprocess.STDOUT, timeout=240)
    text = log.read_text(errors="replace")
    check(name, "no panic", "panicked" not in text, text[-400:])
    check(name, "clean exit", expect_crash or proc.returncode == 0, str(proc.returncode))
    return [l.split("folder-state ", 1)[1] for l in text.splitlines() if "folder-state " in l]
def check(name, what, ok, detail=""):
    print(("  ok   " if ok else "  FAIL ") + f"{name}: {what}" + ("" if ok else f"\n       {detail}"))
    if not ok: failures.append(f"{name}: {what}")
def field(state, key):
    m = re.search(rf"{key}=(\[[^\]]*\]|\"[^\"]*\"|\S*)", state); return m.group(1) if m else None
def pixel(root):
    # A 1x1 PNG, no imaging dependency.
    (root / "pixel.png").write_bytes(bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010806000000" "1f15c4890000000d49444154789c6360f8cfc000000301010018dd8db4" "0000000049454e44ae426082"))

def noop():
    "Opening and looking at files, in every mode, writes nothing."
    crlf = "﻿---\r\na: 1\r\n---\r\n# T\r\n\r\ntext\r\n"
    root = folder("noop", {"README.md": MESSY, "crlf.md": crlf, "sub/other.md": "# Other\n"}); pixel(root)
    before = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
    run("noop", root, f"shot edit\nkey cmd-alt-2\nwait 300\nkey cmd-alt-3\nwait 300\nkey cmd-alt-4\nwait 500\nshot split\nkey cmd-alt-1\nwait 300\nfolder-open {root/'crlf.md'}\nwait 700\nkey cmd-s\nwait 800")
    check("noop", "every byte unchanged", all(p.read_bytes() == b for p, b in before.items()))
    check("noop", "no metadata added", {p for p in root.rglob("*")} - {root / "sub"} == set(before), "")

def typing():
    "Typing in the rendered view patches only those bytes; undo restores them."
    root = folder("typing", {"README.md": MESSY, "sub/other.md": "# Other\n\n## Target here\n"}); pixel(root); f = root / "README.md"
    states = run("typing", root, "folder-caret 0 5\ntype ZZ\nwait 1200\nfolder-state\nshot typed\nkey cmd-z\nwait 1200\nfolder-state")
    check("typing", "ended byte-identical after undo", f.read_text() == MESSY)
    check("typing", "not dirty after undo", states and field(states[-1], "dirty") == "false", str(states))
    root2 = folder("typing2", {"README.md": MESSY}); pixel(root2); f2 = root2 / "README.md"
    run("typing2", root2, "folder-caret 0 5\ntype ZZ\nwait 1200")
    check("typing2", "only the typed bytes changed", f2.read_text() == MESSY.replace("First paragraph", "FirstZZ paragraph"), f2.read_text()[:300])
    root3 = folder("task", {"README.md": MESSY}); pixel(root3); f3 = root3 / "README.md"
    run("task", root3, "folder-caret 1 2\nkey cmd-enter\nwait 1200")
    check("task", "task toggle changes only its marker", f3.read_text() == MESSY.replace("* [ ] star", "* [x] star"), f3.read_text()[100:300])
    root4 = folder("cmds", {"a.md": "# T\n\nkeep *this*\n\nlast\n"}); f4 = root4 / "a.md"
    run("cmds", root4, "folder-caret 1 4\nkey enter\ntype new block\nkey enter\ntype - item\nwait 1200\nshot blocks")
    check("cmds", "new blocks become Markdown beside untouched text", f4.read_text() == "# T\n\nkeep *this*\n\nlast\n\nnew block\n\n- item\n", repr(f4.read_text()))
    root5 = folder("ime", {"a.md": "# T\n\nbody\n"}); f5 = root5 / "a.md"
    run("ime", root5, "folder-caret 0 4\nmark ni\nmark 你\ncommit 你\nwait 1200")
    check("ime", "composed text is written once", f5.read_text() == "# T\n\nbody你\n", repr(f5.read_text()))

def source_mode():
    "Source mode edits the file directly; the rendered view and undo follow."
    original = "---\na: 1\n---\n# Title\n\nbody text\n"
    root = folder("source", {"a.md": original}); a = root / "a.md"
    states = run("source", root, "key cmd-alt-3\nwait 400\nkey cmd-down\ntype extra para\nwait 1400\nfolder-state\nshot source\nkey cmd-alt-1\nwait 600\nfolder-state\nshot rendered\nkey cmd-z\nwait 1400\nfolder-state\nkey cmd-alt-4\nwait 500\nshot split")
    check("source", "source typing saved", field(states[0], "dirty") == "false" and field(states[0], "text_sha") != field(states[-1], "text_sha"), states[0])
    check("source", "rendered view shows it", "extra para" in (work / "source" / "log").read_text(errors="replace") or field(states[1], "revision") != "0", states[1])
    check("source", "undo in the rendered view crosses back", True if field(states[-1], "dirty") == "false" else False, states[-1])
    check("source", "bytes restored", a.read_text() == original, repr(a.read_text()))

def external():
    "Clean buffers follow the disk; dirty ones keep their edits and say so."
    root = folder("external", {"a.md": "# A\n\none\n", "b.md": "# B\n\ntwo\n", "c.md": "# C\n\nthree\n", "d.md": "# D\n\nfour\n"})
    a, b, c, d = (root / n for n in ("a.md", "b.md", "c.md", "d.md"))
    states = run("external", root, f"""folder-open {a}
wait 500
fswrite {a} :: # A changed\\n\\nnew body\\n
wait 1200
folder-state
folder-open {b}
wait 500
folder-caret 0 3
type !
fswrite {b} :: # B external\\n
wait 1500
folder-state
shot conflict
folder-action copy
wait 300
folder-open {c}
wait 500
folder-caret 0 5
type ?
wait 1200
fsrm {c}
wait 1200
folder-state
shot deleted
folder-action keep
wait 800
folder-state
folder-open {d}
wait 500
folder-caret 0 4
type X
fswrite {d} :: # D external\\n
wait 1500
folder-action reload
wait 300
folder-state
folder-action reload
wait 1000
folder-state
""".replace("folder-action copy\nwait 300\n", ""))
    check("external", "clean buffer reloads", 'title="A changed"' in states[0], states[0])
    check("external", "dirty buffer becomes a conflict", field(states[1], "conflict") == "Some(Changed)" and field(states[1], "dirty") == "true", states[1])
    check("external", "the external change is not overwritten", b.read_text() == "# B external\n", b.read_text())
    check("external", "a deletion is reported", field(states[2], "conflict") == "Some(Deleted)", states[2])
    check("external", "restore brings the edits back", c.read_text() == "# C\n\nthree?\n", repr(c.read_text() if c.exists() else None))
    check("external", "reload asks before discarding", field(states[4], "dirty") == "true", states[4])
    check("external", "confirmed reload takes the disk", field(states[5], "dirty") == "false" and d.read_text() == "# D external\n", states[5])

def recovery():
    "Edits that could not be written survive a crash and come back."
    root = folder("recovery", {"a.md": "# A\n\none\n"}); a = root / "a.md"; lib = work / "recovery" / "lib"
    run("recovery", root, f"folder-caret 0 3\ntype !\nfswrite {a} :: # Elsewhere\\n\nwait 1500\nabort", expect_crash=True)
    check("recovery", "disk keeps the external version", a.read_text() == "# Elsewhere\n", a.read_text())
    states = run("recovery", root, "folder-state\nshot recovered", lib=lib)
    check("recovery", "edits are back after the crash", field(states[0], "dirty") == "true", states[0])

def navigation():
    "One history: failed opens don't count, a new destination cuts the forward branch."
    root = folder("nav", {"a.md": "# A\n\n[b](b.md)\n", "b.md": "# B\n\n[c](c.md#deep-part)\n", "c.md": "# C\n\n" + "para\n\n" * 200 + "## Deep part\n\nend\n", "d.md": "# D\n"})
    states = run("nav", root, f"""folder-open {root/'a.md'}
wait 400
folder-state
folder-link b.md
wait 400
folder-link missing.md
wait 400
folder-state
folder-link c.md#deep-part
wait 700
shot anchored
folder-state
key cmd-alt-left
wait 400
folder-state
nav back 800 400
wait 400
folder-state
key cmd-alt-right
wait 400
folder-state
folder-link d.md
wait 400
key cmd-alt-right
wait 400
folder-state
swipe 800 400 120 0
wait 500
folder-state
swipe 800 400 0 200
wait 400
folder-state
key alt-left
wait 300
folder-state
""")
    paths = [field(s, "path") for s in states]
    check("nav", "link opens", paths[1] == "b.md", str(paths))
    check("nav", "failed open stays put", paths[1] == "b.md", str(paths))
    check("nav", "anchor link opens the target", paths[2] == "c.md", str(paths))
    check("nav", "back key", paths[3] == "b.md", str(paths))
    check("nav", "mouse back button", paths[4] == "a.md", str(paths))
    check("nav", "forward key", paths[5] == "b.md", str(paths))
    check("nav", "new destination replaces forward", paths[6] == "d.md", str(paths))
    check("nav", "right swipe goes back once", paths[7] == "b.md", str(paths))
    check("nav", "vertical scroll is not navigation", paths[8] == "b.md", str(paths))
    check("nav", "alt-left still moves by word", paths[9] == "b.md", str(paths))

def renames():
    "A rename takes the open file along, unsaved edits and history included."
    root = folder("rename", {"a.md": "# A\\n\\none\\n".replace("\\\\n", "\\n"), "b.md": "# B\n\nb\n", "c.md": "# C\n\nc\n", "other.md": "# O\n"})
    (root / "a.md").write_text("# A\n\none\n")
    states = run("rename", root, f"""folder-open {root/'a.md'}
wait 400
folder-open {root/'other.md'}
wait 400
folder-open {root/'b.md'}
wait 500
fsmv {root/'b.md'} :: {root/'b2.md'}
wait 1200
folder-state
folder-open {root/'c.md'}
wait 500
folder-caret 0 1
type !
fsmv {root/'c.md'} :: {root/'c2.md'}
wait 2000
folder-state
shot moved
fsmv {root/'a.md'} :: {root/'a2.md'}
wait 800
key cmd-alt-left
wait 500
key cmd-alt-left
wait 500
key cmd-alt-left
wait 700
folder-state
""")
    check("rename", "clean file follows", field(states[0], "path") == "b2.md" and field(states[0], "conflict") == "None", states[0])
    check("rename", "unsaved edits follow and save", field(states[1], "path") == "c2.md" and (root / "c2.md").read_text() == "# C\n\nc!\n", repr((root / "c2.md").read_text()) + states[1])
    check("rename", "nothing is recreated at the old path", not (root / "c.md").exists() and not (root / "b.md").exists())
    check("rename", "history follows a moved file", field(states[2], "path") in ("a2.md",), states[2])

def views():
    "Scroll comes back with history; quitting saves; conflicts ask; themes redraw."
    long = "# Long\n\n" + "\n\n".join(f"Paragraph {i}" for i in range(400)) + "\n"
    root = folder("views", {"long.md": long, "short.md": "# S\n", **{f"n{i}.md": f"# N{i}\n" for i in range(26)}})
    steps = "".join(f"folder-open {root / f'n{i}.md'}\nwait 120\n" for i in range(26))
    states = run("views", root, f"""folder-open {root/'long.md'}
wait 500
wheel 800 400 3000
wait 400
wheel 800 400 3000
wait 400
folder-state
folder-open {root/'short.md'}
wait 400
key cmd-alt-left
wait 500
folder-state
{steps}folder-state
folder-open {root/'long.md'}
wait 800
folder-state
""")
    check("views", "scroll position restored going back", int(field(states[0], "top")) > 0 and field(states[0], "top") == field(states[1], "top"), states[1])
    check("views", "buffers stay bounded", int(field(states[2], "buffers")) <= 24, states[2])
    check("views", "scroll survives eviction", field(states[3], "top") == field(states[0], "top"), states[3])
    q = folder("quit", {"a.md": "# A\n\nbody\n"}); a = q / "a.md"
    run("quit", q, "folder-caret 0 4\ntype Q\nappquit\nwait 500", expect_crash=True)
    check("quit", "quitting (the update path too) saves pending edits", a.read_text() == "# A\n\nbodyQ\n", repr(a.read_text()))
    c = folder("closing", {"a.md": "# A\n\nbody\n"}); ca = c / "a.md"
    states = run("closing", c, f"folder-caret 0 4\ntype Z\nfswrite {ca} :: # Theirs\\n\nwait 1300\nkey cmd-w\nwait 500\nshot save-failed\nfolder-state")
    check("closing", "closing with a conflict keeps the window and the edits", states and field(states[0], "dirty") == "true", str(states))
    check("closing", "the other version is untouched", ca.read_text() == "# Theirs\n", ca.read_text())

def markdown():
    "What real documents contain: alerts, code-span links, math, images, every theme."
    root = folder("md", {"README.md": "# Rich\n\n> [!WARNING]\n> Check `sub/other.md` and the `sub/` folder.\n\n> [!TIP]\n> Energy $E=mc^2$ costs $5 and $10, see https://example.com/x.\n\n![pixel](pixel.png)\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\nneedle in the body text\n\n## Deep Heading\n",
        "sub/other.md": "# Other\n\nThe needle lives here too.\n", "sub/README.md": "# Sub readme\n", "guide.mdx": "# Guide\n\n[home](/README) and [other](/sub/other.md#x)\n"})
    image = bytearray()
    import struct, zlib
    raw = b"".join(b"\x00" + bytes([40, 120, 220, 255]) * 120 for _ in range(60))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    (root / "pixel.png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 120, 60, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    before = (root / "README.md").read_bytes()
    states = run("md", root, f"""wait 1500
shot rich-dark
folder-dark off
wait 1800
shot rich-light
folder-dark on
wait 800
folder-link /sub/
wait 500
folder-state
folder-open {root/'guide.mdx'}
wait 500
folder-link /README
wait 500
folder-state
folder-link guide
wait 500
folder-state
key cmd-j
wait 500
shot palette-recent
type needle
wait 1000
shot palette-content
key down
key down
key enter
wait 800
folder-state
shot found
""")
    check("md", "file untouched", (root / "README.md").read_bytes() == before)
    check("md", "folder link opens its README", field(states[0], "path") == "sub/README.md", states[0])
    check("md", "root-relative link without extension", field(states[1], "path") == "README.md", states[1])
    check("md", "relative link without extension", field(states[2], "path") == "guide.mdx", states[2])
    check("md", "a content hit opens its file", field(states[3], "path") in ("README.md", "sub/other.md"), states[3])

def repository():
    "Staged, unstaged, untracked, deleted, renamed, conflicted; nothing is mutated."
    root = folder("git", {"a.md": "# A\n\none\n", "b.md": "# B\n\nb\n", "gone.md": "# Gone\n", "old name.md": "# Rename me\n\nsame body here\n", "c.md": "base\n"})
    git(root, "init", "-b", "main"); git(root, "add", "."); git(root, "commit", "-m", "base")
    git(root, "checkout", "-q", "-b", "other"); (root / "c.md").write_text("theirs\n"); git(root, "commit", "-qam", "other")
    git(root, "checkout", "-q", "main"); (root / "c.md").write_text("ours\n"); git(root, "commit", "-qam", "ours")
    subprocess.run(["git", "merge", "other"], cwd=root, env=GIT, capture_output=True)
    (root / "a.md").write_text("# A\n\nstaged\n"); git(root, "add", "a.md"); (root / "a.md").write_text("# A\n\nworking\n")
    (root / "gone.md").unlink(); git(root, "mv", "old name.md", "new name.md"); (root / "fresh.md").write_text("# Fresh\n")
    index, config, head = (root / ".git/index").read_bytes(), (root / ".git/config").read_bytes(), (root / ".git/HEAD").read_bytes()
    states = run("git", root, """wait 800
folder-state
folder-git
wait 300
shot panel
folder-diff a.md staged
wait 500
folder-state
shot diff-staged
folder-diff a.md worktree
wait 500
folder-state
shot diff-work
folder-diff fresh.md worktree
wait 500
folder-state
folder-diff new name.md staged
wait 500
folder-state
shot diff-rename
""")
    check("git", "branch and changes", "branch=main" in states[0] and "changes=" in states[0] and "ready" in states[0], states[0])
    check("git", "staged and working diffs differ", 'diff=Some(("a.md", true, ' in states[1] and 'diff=Some(("a.md", false, ' in states[2], states[1])
    check("git", "untracked and renamed diffs load", 'fresh.md", false' in states[3] and 'new name.md", true' in states[4], states[3])
    check("git", "index, config and HEAD untouched", (root / ".git/index").read_bytes() == index and (root / ".git/config").read_bytes() == config and (root / ".git/HEAD").read_bytes() == head)
    linked = work / "git" / "linked"; git(root, "worktree", "add", "-q", "-f", "--detach", str(linked), "HEAD")
    states = run("gitwt", linked, "wait 800\nfolder-state\nshot worktree")
    check("gitwt", "linked worktree, detached HEAD", "worktree=true" in states[0] and "detached=true" in states[0], states[0])
    # Status that fails is a failure, not "no repository" and not "clean".
    broken = folder("gitbroken", {"a.md": "# A\n"}); git(broken, "init", "-b", "main"); git(broken, "add", "."); git(broken, "commit", "-m", "x")
    git(broken, "config", "filter.bad.process", "quiet-md-missing-filter"); git(broken, "config", "filter.bad.required", "true"); (broken / ".gitattributes").write_text("*.md filter=bad\n"); (broken / "a.md").write_text("# B\n")  # same size: Git must run the filter to compare
    states = run("gitbroken", broken, "wait 800\nfolder-state\nfolder-git\nwait 300\nshot failed")
    check("gitbroken", "failed status is reported as failed", "git=[failed" in states[0] and "repo=main" in states[0], states[0])
    plain = folder("gitnone", {"a.md": "# A\n"})
    states = run("gitnone", plain, "wait 500\nfolder-state")
    check("gitnone", "no repository is distinct", "git=[none" in states[0], states[0])

def lfs():
    "LFS filters work with the PATH a Finder launch gets."
    if not any(Path(p).exists() for p in ("/opt/homebrew/bin/git-lfs", "/usr/local/bin/git-lfs")): print("  skip lfs: git-lfs not installed"); return
    root = folder("lfs", {"a.md": "# A\n", "big.bin": b"\0" * 2048})
    git(root, "init", "-b", "main"); git(root, "lfs", "install", "--local"); (root / ".gitattributes").write_text("*.bin filter=lfs diff=lfs merge=lfs -text\n"); git(root, "add", "."); git(root, "commit", "-m", "x")
    (root / "big.bin").write_bytes(b"\1" * 2048)
    config = (root / ".git/config").read_bytes()
    states = run("lfs", root, "wait 1000\nfolder-state")
    check("lfs", "status succeeds through the LFS filter", "git=[ready" in states[0] and "changes=1" in states[0], states[0])
    check("lfs", "config untouched", (root / ".git/config").read_bytes() == config)

def rendering():
    "MDX, big and hostile content stay inside the window at every width."
    long = "x" * 4000
    mdx = "---\ntitle: MDX doc\nsummary: " + "long metadata words " * 80 + "\n---\nimport Widget from './w'\n\n# MDX *doc*\n\n<Widget value={2}>\n  child\n</Widget>\n\nSome {1 + 1} and <Inline /> text.\n\n```mermaid\nnot a diagram at all\n```\n\n```js\n" + long + "\n```\n\n" + long + "\n\n| " + long + " | b |\n|---|---|\n| 1 | 2 |\n"
    root = folder("render", {"doc.mdx": mdx, "big.md": "# Big\n\n" + "\n\n".join(f"Paragraph {i} with some words in it." for i in range(20000)) + "\n", "broken.mdx": "<Component value={\n\nstill readable\n"})
    before = (root / "doc.mdx").read_bytes()
    script = f"folder-open {root/'doc.mdx'}\nwait 800\nfolder-state\n"
    for w in (1280, 800, 480):
        script += f"resize {w} 800\nwait 700\nshot mdx-{w}\nkey cmd-alt-3\nwait 500\nshot source-{w}\nkey cmd-alt-4\nwait 500\nshot split-{w}\nkey cmd-alt-1\nwait 300\n"
    script += f"resize 1280 800\nfolder-meta\nwait 300\nshot meta\nfolder-open {root/'big.md'}\nwait 1500\nfolder-state\nshot big\nfolder-open {root/'broken.mdx'}\nwait 600\nfolder-state\nshot broken\nfolder-menu\nwait 200\nshot menu\nfolder-menu\nkey cmd-j\nwait 500\ntype big\nwait 300\nshot palette\nkey escape\nresize 480 800\nwait 400\nkey cmd-]\nwait 300\nshot overlay\nclick 440 500\nwait 300\nshot overlay-closed\nresize 1280 800\n"
    states = run("render", root, script)
    check("render", "file untouched", (root / "doc.mdx").read_bytes() == before)
    check("render", "large file opens", 'title="Big"' in states[1], states[1])
    check("render", "broken MDX still opens", field(states[2], "path") == "broken.mdx", states[2])

def diagrams():
    "Mermaid renders natively; malformed diagrams show their source."
    root = folder("mermaid", {"a.md": "# Diagrams\n\n```mermaid\nflowchart LR\n  A[Start] --> B{Choice}\n  B -->|yes| C[Done]\n```\n\n```mermaid\nsequenceDiagram\n  Alice->>Bob: Hello\n```\n\n```mermaid\nnot valid\n```\n"})
    before = (root / "a.md").read_bytes()
    run("mermaid", root, "wait 2500\nshot diagrams\nfolder-caret 0 0\nkey right\nwait 800\nshot source-shown")
    check("mermaid", "file untouched", (root / "a.md").read_bytes() == before)

all_ = dict(noop=noop, typing=typing, source_mode=source_mode, external=external, renames=renames, views=views, markdown=markdown, recovery=recovery, navigation=navigation, repository=repository, lfs=lfs, rendering=rendering, diagrams=diagrams)
for name in sys.argv[1:] or all_:
    print(name); all_[name]()
print(f"work: {work}")
if failures: print(f"{len(failures)} FAILED"); sys.exit(1)
print("all passed")
