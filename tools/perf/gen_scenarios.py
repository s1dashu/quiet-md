"""The rest of docs/PERFORMANCE.md's folder-page and memory library, on top of gen_library.py --vault and gen_gallery.py.

usage: python3 tools/perf/gen_scenarios.py <library dir> <work dir>
Writes into the library:
  Data/Long document.md          1 MB of paragraphs under 2,000 headings, a table every tenth section
  Gallery/Many photos.md         the first 30 of Gallery's photos in one note
  Gallery/Photo board.excalidraw the same 30 on a 画板
and <work dir>/big.csv, 30,000 records of 30 columns. The 数据库 is made from it with the CLI (built with
`cargo build --release -p quiet_cli`), in a library of its own, then copied in:
  QUIET_MD_HOME=<work>/t quiet import <work>/big.csv
  QUIET_MD_HOME=<work>/t quiet table field add big Photo --type attachment
  for i in 0..9: QUIET_MD_HOME=<work>/t quiet table update big --where "Score = $i" --set "Photo=<a Gallery photo>"
  cp <work>/t/big.sqlite "<library>/Data/Big table.sqlite"
"""
import csv, json, os, random, sys

lib, work = (os.path.abspath(a) for a in sys.argv[1:3])
r = random.Random(7)
words = 'alpha beta gamma delta sidebar render layout cache import export library folder board table quarter week month 文档 布局 缓存 侧栏 表格'.split()

os.makedirs(os.path.join(lib, 'Data'), exist_ok=True)
out, n = ['# Long document\n'], 0
while n < 1_000_000:
    out.append(f'\n## Section {len(out)}\n')
    for _ in range(8):
        p = ' '.join(r.choice(words) for _ in range(r.randrange(30, 90))) + '.'
        out.append('\n' + p + '\n'); n += len(p)
    if len(out) % 10 == 0:
        out.append('\n| a | b | c |\n|---|---|---|\n' + ''.join(f'| {r.random():.3f} | {r.choice(words)} | {i} |\n' for i in range(12)))
open(os.path.join(lib, 'Data', 'Long document.md'), 'w').write(''.join(out))

gallery = os.path.join(lib, 'Gallery')
open(os.path.join(gallery, 'Many photos.md'), 'w').write('# Many photos\n\nThirty photos in one note.\n' + ''.join(f'\n## Photo {i}\n\n![p{i}](img/p{i}.jpg)\n' for i in range(30)))
def image(i):
    return {"id": f"i{i}", "type": "image", "x": (i % 6) * 620, "y": (i // 6) * 430, "width": 600, "height": 400, "angle": 0, "strokeColor": "transparent",
            "backgroundColor": "transparent", "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 0, "opacity": 100, "groupIds": [],
            "frameId": None, "roundness": None, "seed": r.randint(1, 2**31), "version": 1, "versionNonce": r.randint(1, 2**31), "isDeleted": False,
            "boundElements": [], "updated": 1731576000000, "link": None, "locked": False, "fileId": f"f{i}", "status": "saved", "scale": [1, 1], "crop": None}
files = {f"f{i}": {"mimeType": "image/jpeg", "id": f"f{i}", "path": f"Gallery/img/p{i}.jpg", "created": 1} for i in range(30)}
open(os.path.join(gallery, 'Photo board.excalidraw'), 'w').write(json.dumps({"type": "excalidraw", "version": 2, "source": "quiet", "elements": [image(i) for i in range(30)],
                                                                             "appState": {"viewBackgroundColor": "#ffffff"}, "files": files}))

os.makedirs(work, exist_ok=True)
r = random.Random(1)
cols = ['Title'] + [f'Note {i}' for i in range(1, 9)] + [f'Amount {i}' for i in range(1, 9)] + [f'Day {i}' for i in range(1, 5)] + [f'Tag {i}' for i in range(1, 5)] + ['Done', 'Link', 'Owner', 'Status', 'Score']
with open(os.path.join(work, 'big.csv'), 'w', newline='') as f:
    w = csv.writer(f); w.writerow(cols)
    for i in range(30000):
        row = [f'Item {i + 1} ' + ' '.join(r.choice(words[:17]) for _ in range(3))]
        row += [' '.join(r.choice(words[:17]) for _ in range(r.randrange(2, 12))) for _ in range(8)]
        row += [str(round(r.uniform(-1000, 100000), 2)) for _ in range(8)]
        row += [f'2026-{r.randrange(1, 13):02}-{r.randrange(1, 29):02}' for _ in range(4)]
        row += [r.choice(['red', 'green', 'blue', 'amber', 'violet']) for _ in range(4)]
        row += [r.choice(['true', 'false']), f'https://example.com/{i}', r.choice(['Ann', 'Bo', 'Cy', 'Di']), r.choice(['Todo', 'Doing', 'Done']), str(r.randrange(0, 100))]
        w.writerow(row)
print(f'wrote Data/Long document.md, Gallery/Many photos.md, Gallery/Photo board.excalidraw in {lib}; {work}/big.csv')
