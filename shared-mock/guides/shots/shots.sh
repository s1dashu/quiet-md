#!/bin/zsh
# Retake the 开始指南 screenshots (toolbar, slash) from the guides' own library and, given
# Han Meimei's notes as a folder (HM=), the README's (home, write, board, import) and the guides'
# home.jpg (the README's folder page). Every shot is offscreen (the window stays parked
# and never takes focus); compose.py puts it on the site's meadow with the glass rebuilt.
# usage: [HM=<folder>] shared-mock/guides/shots/shots.sh   (after cargo build --release --features devtools)
#   HM: marketing/mock-users/han-meimei/vault copied to a folder named "Han Meimei", with
#   .obsidian/app.json {"attachmentFolderPath":".attachments"} and extras.py run on it;
#   HM_VAULT: the vault itself, for the import page (default: marketing's, beside this checkout).
set -e
cd "$(dirname "$0")/../../.."
here=shared-mock/guides/shots
work=$(mktemp -d /tmp/quiet-shots.XXXXXX)
compose() { /usr/bin/python3 $here/compose.py "$@"; }
ver=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
# Under QUIET_MD_HOME the settings live in the library's .quiet (Store::settings_dir);
# seen_version: no 已更新到 card over the sidebar.
prefs() { mkdir -p "$1/.quiet"; printf '{"english":true%s,"seen_version":"%s"}\n' "$2" "$ver" > "$1/.quiet/preferences.json"; printf '{"sidebar_width":240}\n' > "$1/.quiet/settings.json"; }
python3 shared-mock/guides/build.py "$work/lib" >/dev/null
prefs "$work/lib" ',"theme":"feishu","feishu":true'
# The copy is named as a new library's folder is, so the sidebar's heading reads Quiet Notes, not lib.
FDL_LIB_NAME="Quiet Notes" python3 tools/harness/run.py $here/steps.txt "$work/lib" "$work/run" >/dev/null 2>&1 || true
for n in toolbar slash; do
  [ -f "$work/run/$n.png" ] || { echo "shots.sh: $n.png missing (see $work/run/log)" >&2; exit 1; }
  # 900×600 with 48pt of meadow around it
  compose "$work/run/$n.png" "$work/$n.png" --sidebar 480 --canvas 1992 1392 --at 96 96
  sips -Z 1600 -s format jpeg -s formatOptions 80 "$work/$n.png" --out "shared-mock/guides/assets/$n.jpg" >/dev/null
done
if [ -n "$HM" ]; then
  hm="$work/hm/Han Meimei"; mkdir -p "$work/hm"; cp -R "$HM" "$hm"; prefs "$hm"
  # The sidebar and the folder page list her folders newest first: these lead, the rest follow.
  python3 -c 'import os,sys,time; r=sys.argv[1]; t=time.time()-86400; o=sys.argv[2:]; o+=sorted(f for f in os.listdir(r) if f not in o and not f.startswith(".") and os.path.isdir(os.path.join(r,f))); [os.utime(os.path.join(r,f),(t-i*3600,)*2) for i,f in enumerate(o)]' "$hm" \
    Index Blue "Signals & Clocks" "Old Medicine" "Manuscripts & Murals" "Maps & Cities" Seeing "Natural History" Journal
  FDL_LIB_NAME="Han Meimei" python3 tools/harness/run.py $here/readme-steps.txt "$hm" "$work/hm-run" >/dev/null 2>&1 || true
  # First launch with the vault where Obsidian says it is, in a stand-in home.
  vault=${HM_VAULT:-${PWD%%/.claude/worktrees/*}/marketing/mock-users/han-meimei/vault}
  fh="$work/home"; v="$fh/Documents/Han Meimei"; mkdir -p "$v/.obsidian" "$fh/Library/Application Support/obsidian" "$work/empty"
  cp -R "$vault/." "$v/"; print '{"attachmentFolderPath":".attachments"}' > "$v/.obsidian/app.json"
  print "{\"vaults\":{\"hm\":{\"path\":\"$v\",\"ts\":1}}}" > "$fh/Library/Application Support/obsidian/obsidian.json"
  HOME="$fh" QUIET_MD_ONBOARDING=1 FDL_LIB_NAME="Quiet Notes" python3 tools/harness/run.py $here/readme-import.txt "$work/empty" "$work/hm-run" >/dev/null 2>&1 || true
  for n in home write board import; do
    [ -f "$work/hm-run/readme-$n.png" ] || { echo "shots.sh: readme-$n.png missing (see $work/hm-run/log)" >&2; exit 1; }
  done
  # shot at 1344pt wide (four columns), shrunk to the 1000pt frame the others have
  compose "$work/hm-run/readme-home.png" "$work/readme-home.png" --scale 0.7440476 --sidebar 357
  compose "$work/hm-run/readme-write.png" "$work/readme-write.png" --sidebar 480
  compose "$work/hm-run/readme-board.png" "$work/readme-board.png" --sidebar 480
  compose "$work/hm-run/readme-import.png" "$work/readme-import.png" --flat '#f2f2f1'
  for n in home write board import; do cwebp -quiet -q 82 "$work/readme-$n.png" -o "docs/images/readme-$n.webp"; done
  sips -Z 1600 -s format jpeg -s formatOptions 80 "$work/readme-home.png" --out shared-mock/guides/assets/home.jpg >/dev/null
fi
echo "$work"
