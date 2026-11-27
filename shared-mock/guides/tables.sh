#!/bin/zsh
# Makes the guides' 数据表 example, src/07a-reading-list.sqlite and its Chinese
# twin, with the quiet CLI (a binary file: build.py copies it as it is).
# usage: shared-mock/guides/tables.sh path/to/quiet
set -eu
quiet=${1:?path to the quiet CLI}
src=${0:A:h}/src
make() { # out title status-field rating-field view-name rows-csv
  local lib; lib=$(mktemp -d)
  QUIET_MD_HOME=$lib $quiet ls > /dev/null  # makes it a library
  $quiet --library $lib new $2 --kind table --csv "$6" > /dev/null
  $quiet --library $lib table field change $2 $3 --type single_select > /dev/null
  $quiet --library $lib table field change $2 $4 --type rating > /dev/null
  $quiet --library $lib table view rename $2 表格视图 $5 > /dev/null
  rm -f $1; cp $lib/$2.sqlite $1
  rm -rf "${lib:?}"
}
make $src/07a-reading-list.sqlite "Reading list" Status Rating "All books" "Book,Author,Status,Rating,Notes
\"Thinking, Fast and Slow\",Daniel Kahneman,Finished,5,\"Familiarity can feel like understanding: pause before deciding when the stakes are high.\"
The Design of Everyday Things,Don Norman,Finished,4,A door that needs a sign is a design problem.
The Mythical Man-Month,Fred Brooks,Reading,,Next: chapter two.
How to Take Smart Notes,Sönke Ahrens,Not started,,"
make $src/zh/07a-reading-list.sqlite 阅读清单 状态 评分 全部 "书名,作者,状态,评分,笔记
思考，快与慢,丹尼尔·卡尼曼,已读完,5,熟悉感很容易被误当成理解：事关重大时，先停一停再做决定。
设计心理学,唐纳德·诺曼,已读完,4,需要贴说明的门，是设计出了问题。
人月神话,弗雷德里克·布鲁克斯,在读,,下一步：读完第二章。
卡片笔记写作法,申克·阿伦斯,未开始,,"
