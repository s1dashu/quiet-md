#!/usr/bin/env python3
"""Make a big folder of code repositories for timing 添加文件夹 (docs/PERFORMANCE.md,
"Adding a code folder").

    gen_code_tree.py <out-dir> [--repos 40] [--scale 1.0] [--project-files]

Each repo: a real .git (git init + a few commits), src/ code, README.md and
docs/*.md, and the build/dependency folders a real checkout has: node_modules
(many packages, each with package.json, lib/*.js, README.md, often CHANGELOG.md),
target/ (Rust), build/, dist/, .venv/, __pycache__/, .next/.
About 7,500 files a repo at scale 1. Deterministic (seeded). Without
--project-files no repo has its Cargo.toml / package.json / pyproject.toml, so
target/, build/ and dist/ read as folders of their own (the harder case).
"""
import argparse, os, random, subprocess, sys

ap = argparse.ArgumentParser()
ap.add_argument("out")
ap.add_argument("--repos", type=int, default=40)
ap.add_argument("--scale", type=float, default=1.0)
ap.add_argument("--project-files", action="store_true")
a = ap.parse_args()
rnd = random.Random(190)
count = {"files": 0, "md": 0}
# git must act on the repo being made, never on one named in the environment.
GIT_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
GIT = ["git", "-c", "user.name=gen", "-c", "user.email=gen@localhost", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]

def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f: f.write(text)
    count["files"] += 1
    if path.endswith(".md"): count["md"] += 1

def n(x): return max(1, int(x * a.scale))

def repo(root, i):
    kind = ["web", "rust", "python", "mixed"][i % 4]
    write(f"{root}/README.md", f"# repo {i}\n\nA {kind} project.\n")
    if a.project_files:
        for marker, kinds in (("package.json", ("web", "mixed")), ("Cargo.toml", ("rust", "mixed")), ("pyproject.toml", ("python", "mixed"))):
            if kind in kinds: write(f"{root}/{marker}", "")
    for d in range(rnd.randint(1, 4)): write(f"{root}/docs/guide-{d}.md", f"# Guide {d}\n\nSome text.\n" * 5)
    ext = {"web": "ts", "rust": "rs", "python": "py", "mixed": "go"}[kind]
    for m in range(n(150)): write(f"{root}/src/mod{m // 20}/file{m}.{ext}", f"// file {m}\n" + "let x = 1;\n" * 30)
    # git: init, then a few commits of the sources.
    subprocess.run(GIT + ["init", "-q", root], check=True, env=GIT_ENV)
    for c in range(5):
        write(f"{root}/src/changes{c}.{ext}", f"// change {c}\n")
        subprocess.run(GIT + ["-C", root, "add", "-A", "src", "README.md", "docs"], check=True, env=GIT_ENV)
        subprocess.run(GIT + ["-C", root, "commit", "-q", "-m", f"change {c}"], check=True, env=GIT_ENV)
    if kind in ("web", "mixed"):
        for p in range(n(600)):
            pk = f"{root}/node_modules/pkg-{p}"
            write(f"{pk}/package.json", '{"name": "pkg-%d", "version": "1.0.0"}\n' % p)
            write(f"{pk}/README.md", f"# pkg-{p}\n\nUsage.\n")
            if p % 3 == 0: write(f"{pk}/CHANGELOG.md", "# Changelog\n\n- 1.0.0\n")
            for j in range(rnd.randint(4, 12)): write(f"{pk}/lib/f{j}.js", "module.exports = 1;\n")
            if p % 10 == 0:
                for j in range(5): write(f"{pk}/node_modules/dep-{j}/index.js", "x\n")
        for j in range(n(400)): write(f"{root}/.next/cache/c{j // 50}/chunk{j}.js", "x\n")
        for j in range(n(300)): write(f"{root}/dist/assets/a{j}.js", "x\n")
    if kind in ("rust", "mixed"):
        for j in range(n(2500)): write(f"{root}/target/debug/deps/lib{j // 5}-{j}.rmeta", "x\n")
        for j in range(n(800)): write(f"{root}/target/debug/.fingerprint/f{j // 4}/dep-{j}", "x\n")
        for j in range(n(400)): write(f"{root}/target/debug/build/b{j // 8}/out/o{j}.rs", "x\n")
    if kind in ("python", "mixed"):
        for j in range(n(3000)): write(f"{root}/.venv/lib/python3.12/site-packages/p{j // 15}/m{j}.py", "x\n")
        for j in range(n(150)): write(f"{root}/.venv/lib/python3.12/site-packages/p{j}/README.md", "# p\n")
        for j in range(n(400)): write(f"{root}/__pycache__/m{j}.cpython-312.pyc", "x\n")
    for j in range(n(500)): write(f"{root}/build/out/o{j // 50}/b{j}.o", "x\n")

os.makedirs(a.out, exist_ok=True)
for i in range(a.repos):
    repo(os.path.join(a.out, f"repo-{i:02d}"), i)
    print(f"\rrepo {i + 1}/{a.repos}  files {count['files']}", end="", file=sys.stderr)
print(file=sys.stderr)
print(f"{count['files']} files written ({count['md']} .md), plus .git objects")
