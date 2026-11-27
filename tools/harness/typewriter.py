"""打字机模式: the caret's line stays in the middle of the page while one writes.

Build with --release --features devtools, then run this file. FDL_BIN selects
another build; TYPEWRITER_CASES selects comma-separated cases. Every library,
script and log stays in a temporary directory; each run quits the app.

Cases: typing, enter, arrows, start, end, wheel (a manual scroll stays until
the next keystroke), click (a click doesn't scroll), select (a selection
reveals as usual), off (the switch off: no centring), shortcut (⌘⇧T turns it
on), low (克制 motion jumps), perf (typing in a long document stays fast),
focus (段落聚焦 fades the other paragraphs, light and dark, and keeps a selection clear),
find (a hit comes to the middle), resize (a resize keeps a centred line there, not a
scrolled page), paste (a big paste is there at once), ime (composing keeps the page
still), focus_key (⌘⇧F toggles 段落聚焦, remembered).
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-typewriter-"))
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
W, H = 1200, 850
LINE = 30  # a body line is about 26px; the middle may be off by up to half of one


def fixture(name, typewriter=True, motion="high", paragraphs=80, long=False, focus=False, appearance="light"):
    source = work / (name + "-source")
    (source / "docs").mkdir(parents=True)
    (source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
    (source / "preferences.json").write_text(json.dumps({
        "theme": "craft", "appearance": appearance, "paragraph_focus": focus, "english": True, "seen_version": "9.9.9",
        "auto_update": False, "quick_hotkey": False, "typewriter": typewriter, "motion": motion,
    }))
    xml = '<title>Typewriter</title>\n'
    body = ("A long paragraph that wraps over several lines of the page so the document is heavy. " * 6) if long else "Paragraph"
    xml += ''.join(f'<p>{body} {i:03d}{" MIDDLE_MARK" if i == paragraphs // 2 else ""}</p>\n' for i in range(paragraphs))
    (source / "docs" / f"{doc_id}.xml").write_text(xml)
    return source


def run(name, steps, **kw):
    source = fixture(name, **kw)
    folder = work / name
    script = work / (name + ".txt")
    script.write_text(f"resize {W} {H}\nwait 1500\nmove 1150 820\n" + steps + "\nquit\n")
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
                   env=dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1"), check=True, timeout=120)
    log = (folder / "log").read_text(errors="replace")
    carets = [(float(y), float(h)) for y, h in re.findall(r"devtools: caret (-?[\d.]+) of ([\d.]+)", log)]
    return carets, log, folder


def page_top(log):
    return float(re.findall(r"devtools: caret -?[\d.]+ of [\d.]+ at ([\d.]+)", log)[-1])


def ink(folder, shot, y):
    """How far the text on the line at window y (points) stands from the page: 0–255."""
    from PIL import Image
    im = Image.open(folder / (shot + ".png")).convert("L")
    k = im.width / W
    strip = im.crop((round(330 * k), round((y - 10) * k), round(900 * k), round((y + 10) * k)))
    px = sorted(strip.get_flattened_data())
    bg = px[len(px) // 2]
    return max(abs(v - bg) for v in px)


# Puts the caret in the middle of the document (find, then close the bar),
# then scrolls it off the page's middle, where find left it.
to_middle = "key cmd-f\ntype MIDDLE_MARK\nkey escape\nkey right\nwait 500\nwheel 700 400 -200\nwait 500\n"
settle = "wait 600\ncaret\n"


def centred(c, what, folder):
    y, h = c
    assert abs(y - h / 2) <= LINE, f"{what}: caret at {y:.0f} of {h:.0f}, not the middle; see {folder}"


def near_top(c, what, folder):
    y, h = c
    assert y < h / 2 - 3 * LINE, f"{what}: caret at {y:.0f} of {h:.0f}, expected near the top; see {folder}"


def case_typing():
    carets, _, f = run("typing", to_middle + "type abc\n" + settle + "type  more words\n" + settle)
    for c in carets: centred(c, "typing", f)


def case_enter():
    carets, _, f = run("enter", to_middle + "type x\n" + settle + "key enter\nkey enter\nkey enter\ntype new line\n" + settle)
    for c in carets: centred(c, "enter", f)


def case_arrows():
    carets, _, f = run("arrows", to_middle + "type x\n" + settle + "key down\nkey down\nkey down\nkey down\n" + settle + "key up\n" + settle)
    for c in carets: centred(c, "arrows", f)


def case_start():
    # The page can't scroll above its start: the first lines stay where they are.
    carets, _, f = run("start", to_middle + "type x\n" + settle + "key cmd-up\n" + settle + "type y\n" + settle)
    centred(carets[0], "start (before)", f)
    near_top(carets[1], "start", f)
    near_top(carets[2], "start, typing", f)


def case_end():
    # The page pads its end: the last line comes up to the middle too.
    carets, _, f = run("end", to_middle + "key cmd-down\n" + settle + "type z\n" + settle + "key enter\ntype last\n" + settle)
    for c in carets: centred(c, "end", f)


def case_wheel():
    carets, _, f = run("wheel", to_middle + "type x\n" + settle + "wheel 700 400 -250\n" + settle
                       + "wait 800\ncaret\ntype y\n" + settle)
    centred(carets[0], "wheel (before)", f)
    assert abs(carets[1][0] - carets[0][0]) > 200, f"wheel didn't scroll: {carets}; see {f}"
    assert abs(carets[2][0] - carets[1][0]) < 1, f"the page was pulled back without a keystroke: {carets}; see {f}"
    centred(carets[3], "wheel, then typing", f)


def case_click():
    # A click puts the caret where it lands and leaves the page alone.
    carets, _, f = run("click", to_middle + "type x\n" + settle + "click 600 200\n" + settle)
    centred(carets[0], "click (before)", f)
    assert carets[1][0] < carets[1][1] / 2 - 3 * LINE, f"a click scrolled the page: {carets}; see {f}"


def case_select():
    # Shift+↓ across paragraphs selects blocks (Feishu): the page reveals the
    # selection as usual and doesn't centre it, so the caret's line stays put.
    carets, _, f = run("select", to_middle + "type x\n" + settle + "key shift-down\nkey shift-down\n" + settle)
    centred(carets[0], "select (before)", f)
    assert abs(carets[1][0] - carets[0][0]) < 1, f"the selection scrolled the page: {carets}; see {f}"


def case_off():
    carets, _, f = run("off", to_middle + "wait 600\ncaret\ntype abc\n" + settle, typewriter=False)
    assert abs(carets[1][0] - carets[0][0]) < 1, f"typing scrolled with the switch off: {carets}; see {f}"
    assert abs(carets[1][0] - carets[1][1] / 2) > LINE, f"find already centres the hit; pick another mark: {carets}; see {f}"


def case_shortcut():
    carets, log, f = run("shortcut", to_middle + "wait 600\ncaret\nkey cmd-shift-t\n" + settle + "type a\n" + settle
                         + "key cmd-shift-t\nkey down\nkey down\n" + settle, typewriter=False)
    centred(carets[1], "shortcut on", f)
    centred(carets[2], "shortcut on, typing", f)
    assert carets[3][0] > carets[2][0] + LINE, f"⌘⇧T didn't turn it off: {carets}; see {f}"
    saved = json.loads(next((f / "lib").rglob("preferences.json")).read_text())
    assert saved.get("typewriter") is False, f"the switch isn't remembered: {saved}; see {f}"


def case_low():
    # 克制: no glide, the page is in place on the next frame.
    carets, _, f = run("low", to_middle + "type x\nwait 50\ncaret\nkey enter\nwait 50\ncaret\n", motion="")
    for c in carets: centred(c, "low motion", f)


def bench_mean(log):
    m = re.findall(r'bench "type a" x\d+: frame mean ([\d.]+)', log)
    assert m, "no bench line"
    return float(m[-1])


def case_perf():
    # ~1M characters: a keystroke with the switch on costs what it costs off.
    steps = to_middle + "type x\nwait 600\nbench 40 type a\n"
    on = bench_mean(run("perf-on", steps, paragraphs=2000, long=True)[1])
    off = bench_mean(run("perf-off", steps, paragraphs=2000, long=True, typewriter=False)[1])
    print(f"perf: frame mean {on:.2f}ms on, {off:.2f}ms off")
    assert on < max(off * 1.5, off + 4), f"typing is slower with 打字机模式: {on:.2f} vs {off:.2f} ms"


def case_focus():
    # 段落聚焦: the caret's paragraph keeps its ink, the others fade (both looks);
    # a selection over three paragraphs keeps all three clear; the title alone at the start.
    for look in ("light", "dark"):
        steps = to_middle + "type x\n" + settle + "shot mid\nkey shift-down\nkey shift-down\nwait 600\nshot sel\n"
        carets, log, f = run("focus-" + look, steps, focus=True, appearance=look)
        y = page_top(log) + carets[0][0]
        lit, above, below3 = ink(f, "mid", y), ink(f, "mid", y - 2 * 35), ink(f, "mid", y + 3 * 35)
        assert lit > 1.8 * max(above, below3), f"{look}: the other paragraphs don't fade ({lit} vs {above}, {below3}); see {f}"
        sel = [ink(f, "sel", y + i * 35) for i in range(3)]
        assert min(sel) > 1.8 * ink(f, "sel", y + 4 * 35), f"{look}: a selected paragraph faded {sel}; see {f}"
    carets, log, f = run("focus-off", to_middle + "type x\n" + settle + "shot mid\n")
    y = page_top(log) + carets[0][0]
    assert ink(f, "mid", y + 3 * 35) > 0.7 * ink(f, "mid", y), f"faded with the switch off; see {f}"


def case_find():
    # A find hit comes to the middle, where the caret will write.
    carets, _, f = run("find", "key cmd-f\ntype MIDDLE_MARK\nwait 700\nkey escape\nkey right\n" + settle)
    centred(carets[0], "find", f)


def case_resize():
    # The window resized under a centred caret: the line stays in the middle;
    # scrolled away by the wheel first, the page stays where it was put.
    carets, log, f = run("resize", to_middle + "type x\n" + settle + f"resize {W} 600\n" + settle + f"resize {W} {H}\n" + settle
                         + "wheel 700 400 -150\n" + settle + f"resize {W} 700\n" + settle)
    for i in (1, 2): centred(carets[i], f"resize {i}", f)
    assert abs(carets[4][0] - carets[3][0]) < 1, f"a resize pulled a scrolled page back: {carets}; see {f}"


def case_paste():
    # A big paste lands the caret far below: the page is there at once, not gliding a long way.
    clip = work / "big.txt"
    clip.write_text("\n".join(f"Pasted line {i}" for i in range(150)))
    carets, _, f = run("paste", to_middle + "type x\n" + settle + f"clip text {clip}\nkey cmd-v\nwait 60\ncaret\n" + settle)
    centred(carets[1], "paste, next frames", f)
    centred(carets[2], "paste", f)


def case_ime():
    # A word being composed keeps the page still; once committed, the line comes to the middle.
    carets, _, f = run("ime", to_middle + "wait 300\ncaret\nmark nihao\n" + settle + "commit 你好\n" + settle)
    assert abs(carets[1][0] - carets[0][0]) < 1, f"composing moved the page: {carets}; see {f}"
    centred(carets[2], "ime committed", f)


def case_focus_key():
    # ⌘⇧F turns 段落聚焦 on and off, remembered.
    carets, log, f = run("focus-key", to_middle + "type x\n" + settle + "key cmd-shift-f\nwait 600\nshot on\nkey cmd-shift-f\nwait 600\nshot off\n")
    y = page_top(log) + carets[0][0]
    assert ink(f, "on", y) > 1.8 * ink(f, "on", y + 3 * 35), f"⌘⇧F didn't fade the others; see {f}"
    assert ink(f, "off", y + 3 * 35) > 0.7 * ink(f, "off", y), f"⌘⇧F didn't turn it off; see {f}"
    saved = json.loads(next((f / "lib").rglob("preferences.json")).read_text())
    assert saved.get("paragraph_focus") is False, f"not remembered: {saved}; see {f}"


CASES = {n[5:]: f for n, f in globals().items() if n.startswith("case_")}

if __name__ == "__main__":
    picked = [c for c in os.environ.get("TYPEWRITER_CASES", ",".join(CASES)).split(",") if c]
    for name in picked:
        CASES[name]()
        print("ok", name)
    print("all passed;", work)
