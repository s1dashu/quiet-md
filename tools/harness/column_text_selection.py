"""Verify mouse text selection inside top, center and bottom aligned grid columns.

Run after building with devtools. Uses temporary libraries and never reads the
system clipboard. Pass an output directory to retain screenshots and saved XML.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET


def main():
    """Replay a text replacement and check the saved column keeps exactly two paragraphs."""
    repo = Path(__file__).resolve().parents[2]
    work = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="quiet-column-text-"))
    source = work / "source"
    (source / "docs").mkdir(parents=True, exist_ok=True)
    (source / "assets").mkdir(exist_ok=True)
    shutil.copy2(repo / "docs/images/home.png", source / "assets/sample.png")
    doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
    (source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1, "pinned": True}}))
    (source / "preferences.json").write_text(json.dumps({
        "english": True, "feishu": True, "seen_version": "0.1.9",
        "auto_update": False, "quick_hotkey": False,
    }))
    for alignment, y in [("top", 220), ("center", 296), ("bottom", 372)]:
        (source / "docs" / f"{doc_id}.xml").write_text(
            '<title>Column text selection</title><p>Outside text selection baseline.</p>'
            f'<grid data-quiet-valign="{alignment}"><column width-ratio="0.5">'
            '<p>SELECT THIS TEXT</p><p>Second paragraph stays here.</p></column>'
            '<column width-ratio="0.5"><img src="assets/sample.png" width="220" height="180" '
            'caption="Caption text selection" align="center"/></column></grid><p>After the grid.</p>'
        )
        script = work / f"{alignment}.txt"
        script.write_text(
            'wait 1000\nresize 1200 900\nwait 200\nclick 120 135\n'
            f'drag 346 {y} 490 {y}\nwait 200\nshot selected\ntype REPLACED\nwait 700\nshot edited\n'
        )
        output = work / alignment
        subprocess.run([sys.executable, str(repo / "tools/harness/run.py"), str(script), str(source), str(output)],
                       cwd=repo, check=True, timeout=45)
        saved = (output / "lib/docs" / f"{doc_id}.xml").read_text()
        document = ET.fromstring(f"<doc>{saved}</doc>")
        paragraphs = [p.text for p in document.findall("./grid/column")[0].findall("p")]
        assert paragraphs == ["REPLACED", "Second paragraph stays here."], (alignment, paragraphs)
        assert document.find("./grid").get("data-quiet-valign", "top") == alignment
        print(f"{alignment}: text selection and replacement passed", flush=True)
    print(f"Screenshots and saved documents: {work}")


if __name__ == "__main__":
    main()
