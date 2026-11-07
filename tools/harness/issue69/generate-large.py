"""Create a new synthetic performance library; refuse to overwrite a directory."""
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=False)
(root / "docs").mkdir()
ident = "aaaaaaaa-0000-0000-0000-000000000069"
(root / "index.json").write_text(json.dumps({ident: {"created": 1793966400, "modified": 1793966400, "opened": 1793966400, "pinned": False}}))
text = "<title>Issue 69 性能测试</title>\n<p>before <latex>x^2</latex> after</p>\n" + ("<p>" + "sample text " * 83 + "end</p>\n") * 1000
(root / "docs" / f"{ident}.xml").write_text(text)
print(f"{len(text)} characters: {root}")
