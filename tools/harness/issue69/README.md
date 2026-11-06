# Issue #69 experimental checks

Build with `cargo build --release --features experimental-inline-math,devtools`.
On a Mac with Command Line Tools only, add `gpui/runtime_shaders` to the build features. Do not install extra software or change security settings.

Run from the repo root with `QUIET_MD_INLINE_MATH=1 FDL_NOBLINK=1`:

- `python3 tools/harness/run.py tools/harness/issue69/edit-flow.txt tools/harness/issue69/library /tmp/issue69-flow-NEW`
- `python3 tools/harness/run.py tools/harness/issue69/mouse-selection.txt tools/harness/issue69/library /tmp/issue69-mouse-NEW`
- Run `reopen.txt` against the saved `lib` directory from the edit flow.

Use a new work directory; run.py copies its fixture there. It never uses the
normal library. No activate, open, real clipboard copy or system settings.
`math-check` asserts source, focus, marked state, offsets and canonical XML.
The `mark`/`commit` commands simulate composition; a real input method still
needs human testing. Cross-body selections keep the whole equation object.
The large-perf script needs a synthetic library with a formula in its first
paragraph and about one million characters in following paragraphs; it does
not establish performance for every document or screen presentation latency.

Crash regression (source-offset aliasing and stale projected layouts):
- `left-navigation.txt`: from the suffix, enter with Left, traverse/leave both ways; empty, short, longer and Unicode sources. `model_head` and `origin` assert exact document coordinates.
- `drag-navigation.txt`: source selection, cross-body drag, then horizontal/vertical/line-edge navigation and save. Every math-check also validates UTF-8 model caret boundaries.
Run each through run.py with a fresh isolated work directory, as above.
- `dollars-render.txt`: exact `$$$$`, Left twice, `f(x^2)` workflow; full `$$f(x^2)$$` typing and multi-character commit; assert saved source, real math layout success, and entry/exit from both directions.
- `dollars-pointer.txt` repeats the four-dollar workflow using a pointer click between the delimiters. Waits before pointer/focus assertions explicitly draw pending frames when macOS stops the display link for an off-screen test window.
- `dollars-native.txt` sends macOS keyDown/keyUp events for dollar signs and `f(x^2)`, logs the NSTextInputContext input-source identifier, and asserts real rendering plus bidirectional navigation. It neither activates the window nor changes the selected input source.

- `read-only.txt` verifies that arrow keys keep equations rendered in reading mode and that typed input does not edit the document.
