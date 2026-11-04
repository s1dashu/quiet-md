# Gates: Folder workspace

OWNS: src/**, crates/core/**, Cargo.toml, Cargo.lock, docs/**, tools/harness/**, assets/pele/**, THIRD_PARTY.md, GATES.md, shared-mock/guides/assets/**, shared-mock/library/**

Scope: Source-preserving folder editing, native Markdown and partial MDX preview, Git status/diffs, navigation and gestures, and bundled Pele diagrams.

- [x] G1: Folder files remain authoritative, preserve exact unchanged bytes, and reject observed external-change overwrites; source undo and recovery work.
  CHECK: cargo test --release -p fdl_core workspace_files
  EXPECT: test result: ok
  EVIDENCE: /tmp/quiet-mdx-check-final.log: workspace_files test passed, including missing ancestor recovery. Save checks before replacement; this is not an atomic cross-process compare-and-swap.

- [x] G2: Git discovery, both status columns, diffs, and worktrees work on temporary repositories.
  CHECK: cargo test --release -p fdl_core workspace_git
  EXPECT: test result: ok
  EVIDENCE: /tmp/quiet-mdx-check.log: workspace_git test passed, including a temporary worktree and untracked text diff.

- [x] G3: Markdown/MDX preview preserves unsupported source and Pele renders bundled Mermaid without network access.
  CHECK: cargo test --release workspace_preview
  EXPECT: test result: ok
  EVIDENCE: /tmp/quiet-mdx-check.log: preview, media copy, and Pele rendering tests passed; flowchart, sequence and class diagrams produced visible pixels. App screenshot also showed a diagram; physical network isolation was not tested.

- [x] G4: History and gesture guards handle back, forward, branching, and vertical scrolling.
  CHECK: cargo test --release workspace_navigation
  EXPECT: test result: ok
  EVIDENCE: /tmp/quiet-mdx-check.log: workspace_navigation unit test passed. Physical mouse and trackpad input are not verified by this test.

- [x] G5: Repository checks pass, including XML round trips and clippy.
  CHECK: scripts/check.sh
  EXPECT: Finished
  EVIDENCE: zsh, cwd /Users/zeus/hub/quiet-mdx, scripts/check.sh exit 0; matched "Bundled guide assets match their sources" and release clippy "Finished". 87 core and 43 application tests passed; 2 core and 1 application tests ignored. Log /tmp/quiet-mdx-check-final.log SHA-256 dac5df9c6a14a1314cba17c5088e096eeaecddbb457419ccbdee6ec1ff5ab532. A separate generated-guide round-trip also passed; /tmp/quiet-mdx-guide-roundtrip.log SHA-256 6561c3ba7dfa0a3f3ee2235bdf7328bf44aaf5a302e639e6e97b9b7742f16495.

- [x] G6: Harness exercises folder selection, source editing, preview, Git, navigation and diagram display against temporary fixtures.
  CHECK: zsh, cwd /Users/zeus/hub/quiet-mdx: python3 tools/harness/folder_smoke.py
  EXPECT: PASS: folder open, Git fixture, navigation, undo/redo and external-change protection
  EVIDENCE: exit 0; matched PASS. Log /var/folders/w4/_pd1qs2x6z96c1rzpc6y8gz80000gn/T/quiet-folder-smoke-zpertp_s/run/log; SHA-256 ada2b149cc6e9877c0273a33321ad9337982ee91cfc80e3eb04303e81cd566a7. Events were injected by the harness. The diagram was visually inspected in the initial screenshot.
