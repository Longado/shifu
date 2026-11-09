#!/usr/bin/env bash
# 集成冒烟：专用 socket + 临时 SHIFU_HOME，用假 agent，不碰真 claude/codex/默认 tmux。
set -u
cd "$(dirname "$0")/.."
BIN="${SHIFU_BIN:-$PWD/target/release/shifu}"
[ -x "$BIN" ] || { echo "先 cargo build --release"; exit 1; }

export SHIFU_SOCKET=shifu-smoke
TMP="$(mktemp -d)"
export SHIFU_HOME="$TMP/home"
TMUX_BIN="${SHIFU_TMUX:-/opt/homebrew/bin/tmux}"
T() { "$TMUX_BIN" -L "$SHIFU_SOCKET" "$@"; }
# 侧栏只在自己那一页正显示在某个终端上时才刷新，所以要看侧栏就得真有个终端 attach 着：
# 用另一个 tmux 服务的窗格当「终端」，里面跑一个 attach 到冒烟服务的客户端。
OUTER() { "$TMUX_BIN" -L shifu-smoke-outer -f /dev/null "$@"; }
attach_client() { OUTER kill-server 2>/dev/null; OUTER new-session -d -x 160 -y 40 "env TMUX= '$TMUX_BIN' -L '$SHIFU_SOCKET' attach"; sleep 1; }
cleanup() { OUTER kill-server 2>/dev/null; T kill-server 2>/dev/null; rm -rf "$TMP"; }
trap cleanup EXIT
T kill-server 2>/dev/null  # 上一次没收拾干净的冒烟服务；环境变量不同的旧服务会让后面的检查白跑

fail=0
ok()   { echo "ok   - $1"; }
bad()  { echo "FAIL - $1"; fail=1; }
check() { # check <描述> <命令...>
  local desc="$1"; shift
  if "$@"; then ok "$desc"; else bad "$desc"; fi
}
wait_for() { # wait_for <描述> <命令...>，最多等 10 秒
  local desc="$1"; shift
  for _ in $(seq 1 50); do "$@" && { ok "$desc"; return; }; sleep 0.2; done
  bad "$desc"
}
ls_has() { "$BIN" ls | grep -q "$1"; }

# 假的 claude / codex：记下参数和环境，然后挂着
mkdir -p "$TMP/bin" "$TMP/rec"
for prog in claude codex; do
  cat > "$TMP/bin/$prog" <<SH
#!/bin/sh
{ for a in "\$@"; do printf '%s\n' "\$a"; done; } > "$TMP/rec/$prog.args"
env > "$TMP/rec/$prog.env"
sleep 600
SH
  chmod +x "$TMP/bin/$prog"
done
export PATH="$TMP/bin:$PATH"
# 模拟「从一个 Claude Code / Codex 会话里启动」：这些变量不该漏进 agent
export CLAUDECODE=1 CLAUDE_CODE_CHILD_SESSION=1 CODEX_THREAD_ID=abc CODEX_HOME="$TMP/codex-home"

DIR="$TMP/x/中文 目录"
mkdir -p "$DIR"

echo "== 没有 agent"
"$BIN" ls | grep -q "还没有 agent" && ok "空列表提示" || bad "空列表提示"

echo "== 开假 agent（目录含中文和空格）"
"$BIN" new sh "$DIR" -- -c 'sleep 600'
check "ls 看到 sh 在运行中" ls_has "^sh .*运行中"
PANE="$(python3 -c "import json,sys;print(json.load(open('$SHIFU_HOME/agents/sh.json'))['pane_id'])")"
WIN="$(python3 -c "import json,sys;print(json.load(open('$SHIFU_HOME/agents/sh.json'))['window_id'])")"
check "窗格目录是中文空格目录" bash -c "[ \"\$('$TMUX_BIN' -L $SHIFU_SOCKET display-message -p -t '$PANE' '#{pane_current_path}')\" = \"\$(cd '$DIR' && pwd -P)\" ]"
check "窗口里有侧栏窗格（共 2 个）" bash -c "[ \"\$('$TMUX_BIN' -L $SHIFU_SOCKET list-panes -t '$WIN' | wc -l | tr -d ' ')\" = 2 ]"
check "agent 文件里目录正确" grep -q '中文 目录' "$SHIFU_HOME/agents/sh.json"

echo "== 模拟 hook"
SHIFU_ID=sh "$BIN" hook claude PermissionRequest < /dev/null > "$TMP/hook.out" 2>&1
check "hook 没有任何输出" bash -c "[ ! -s '$TMP/hook.out' ]"
check "ls 显示等你批准" ls_has "^sh .*等你批准"

echo "== 侧栏渲染"
SIDE="$(T list-panes -t "$WIN" -F '#{pane_id} #{pane_current_command}' | awk '$2=="shifu"{print $1}' | head -1)"
attach_client
T select-window -t "$WIN"
sleep 2
CAP="$(T capture-pane -p -t "$SIDE")"
echo "$CAP" | sed 's/^/  | /'
check "侧栏标题" bash -c "echo '$CAP' | grep -q shifu"
check "侧栏显示 agent 名和状态" bash -c "echo '$CAP' | grep -q '等你批准'"

echo "== 同目录再开一个 → 重名加后缀"
"$BIN" new sh "$DIR" -- -c 'sleep 600'
check "出现 sh-2" ls_has "^sh-2 "

echo "== 假 claude：hooks 文件与参数"
"$BIN" new claude "$DIR"
wait_for "假 claude 收到参数" test -f "$TMP/rec/claude.args"
check "带 --settings" grep -qx -- '--settings' "$TMP/rec/claude.args"
check "settings 指向 claude-hooks.json" grep -qx "$SHIFU_HOME/claude-hooks.json" "$TMP/rec/claude.args"
check "SHIFU_ID 传进去了" grep -q '^SHIFU_ID=claude$' "$TMP/rec/claude.env"
check "TMUX_PANE 传进去了" grep -q '^TMUX_PANE=%' "$TMP/rec/claude.env"
check "CLAUDECODE 已清掉" bash -c "! grep -q '^CLAUDECODE=' '$TMP/rec/claude.env'"
check "CLAUDE_CODE_* 已清掉" bash -c "! grep -q '^CLAUDE_CODE_' '$TMP/rec/claude.env'"
check "CODEX_THREAD_ID 已清掉" bash -c "! grep -q '^CODEX_THREAD_ID=' '$TMP/rec/claude.env'"
check "CODEX_HOME 保留" grep -q "^CODEX_HOME=$TMP/codex-home\$" "$TMP/rec/claude.env"
HOOK_CMD="$(python3 -c "import json;print(json.load(open('$SHIFU_HOME/claude-hooks.json'))['hooks']['Stop'][0]['hooks'][0]['command'])")"
SHIFU_ID=claude sh -c "$HOOK_CMD" < /dev/null
check "hooks 文件里的 Stop 命令能用" ls_has "^claude .*等你回复"

echo "== 假 codex：七个 -c 覆盖"
"$BIN" new codex "$DIR"
wait_for "假 codex 收到参数" test -f "$TMP/rec/codex.args"
check "七个 -c" bash -c "[ \"\$(grep -cx -- '-c' '$TMP/rec/codex.args')\" = 7 ]"
check "第一个覆盖是 SessionStart" bash -c "sed -n 2p '$TMP/rec/codex.args' | grep -q '^hooks.SessionStart='"
check "没有 bypass 参数" bash -c "! grep -q bypass '$TMP/rec/codex.args'"

echo "== 杀掉 agent 进程"
PID="$(T display-message -p -t "$PANE" '#{pane_pid}')"
kill -9 "$PID"
wait_for "ls 显示 sh 已退出" ls_has "^sh .*已退出"

echo "== 不存在的目录"
"$BIN" new sh "$TMP/不存在" 2> "$TMP/err.txt"; rc=$?
check "退出码非 0" test "$rc" -ne 0
check "报错说明目录不可用" grep -q '不可用' "$TMP/err.txt"

echo "== jump"
"$BIN" jump
check "jump 没报错" test $? -eq 0

# ---------------------------------------------------------------- v1.1
jf() { python3 -c "import json;print(json.load(open('$SHIFU_HOME/agents/$1.json'))['$2'])"; } # jf <agent> <字段>
win_of() { T display-message -p -t "$1" '#{window_id}'; }
opt() { T show-options -p -v -t "$1" @shifu_status; }
sidebar_of() { T list-panes -t "$1" -F '#{pane_index} #{pane_id}' | awk '$1==0{print $2}'; }
pane_info() { T display-message -p -t "$1" "$2"; }
has_window() { T list-windows -F '#{window_id}' | grep -qx "$1"; }
has_pane() { T list-panes -a -F '#{pane_id}' | grep -qx "$1"; }
not() { ! "$@"; }
same_row_right_of() { # same_row_right_of <右边的窗格> <左边的窗格>
  [ "$(pane_info "$1" '#{pane_left}')" -gt "$(pane_info "$2" '#{pane_left}')" ] && [ "$(pane_info "$1" '#{pane_top}')" = "$(pane_info "$2" '#{pane_top}')" ]
}
same_column_below() { # same_column_below <下面的窗格> <上面的窗格>
  [ "$(pane_info "$1" '#{pane_top}')" -gt "$(pane_info "$2" '#{pane_top}')" ] && [ "$(pane_info "$1" '#{pane_left}')" = "$(pane_info "$2" '#{pane_left}')" ]
}
within_one() { # within_one <格式> <窗格> <窗格>：两个数差不超过 1
  local a b; a="$(pane_info "$2" "$1")"; b="$(pane_info "$3" "$1")"
  [ $((a - b)) -le 1 ] && [ $((b - a)) -le 1 ]
}
refuses() { # refuses <页里的窗格> <报错里的字> <act 参数...>：act 失败且报错带指定的字
  local pane="$1" word="$2"; shift 2
  local err; err="$(TMUX_PANE="$(sidebar_of "$(win_of "$pane")")" "$BIN" act "$@" 2>&1 >/dev/null)"; local rc=$?
  [ "$rc" -ne 0 ] && echo "$err" | grep -q "$word"
}
sidebar_shifu() { [ "$(T list-panes -t "$1" | wc -l | tr -d ' ')" = 2 ] && [ "$(pane_info "$(sidebar_of "$1")" '#{pane_current_command}')" = shifu ]; }
TMUXVAR="$(T display-message -p '#{socket_path},#{pid},0')"
hook_as() { # hook_as <agent> <事件> [stdin]：带着 TMUX/TMUX_PANE 跑 hook，和 agent 里的真实环境一样
  printf '%s' "${3:-}" | SHIFU_ID="$1" TMUX="$TMUXVAR" TMUX_PANE="$(jf "$1" pane_id)" "$BIN" hook claude "$2"
}
act_in() { # act_in <页里任意窗格> <动作...>：当作用户正在看那一页
  local pane="$1"; shift
  TMUX_PANE="$(sidebar_of "$(win_of "$pane")")" "$BIN" act "$@"
}

echo "== v1.1：窗格选项、标签栏圆点、tally"
mkdir -p "$TMP/pa" "$TMP/pb" "$TMP/pc"
"$BIN" new sh "$TMP/pa" -- -c 'sleep 600'
"$BIN" new sh "$TMP/pb" -- -c 'sleep 600'
A=sh-pa; B=sh-pb
PA="$(jf $A pane_id)"; PB="$(jf $B pane_id)"
check "没有 hook 的命令，new 直接把 @shifu_status 设成 running" test "$(opt "$PA")" = running
check "recent.json 记了目录" grep -q '/pb"' "$SHIFU_HOME/recent.json"
hook_as $A PermissionRequest > "$TMP/hook2.out" 2>&1
check "带 TMUX 的 hook 也没有任何输出" bash -c "[ ! -s '$TMP/hook2.out' ]"
check "hook 把 @shifu_status 改成 needs_approval" test "$(opt "$PA")" = needs_approval
hook_as $A PostToolUse; check "批准后变 running" test "$(opt "$PA")" = running
hook_as $A PermissionRequest
hook_as $B Stop
check "B 变 needs_reply" test "$(opt "$PB")" = needs_reply
FMT_A="$(T display-message -p -t "$PA" '#{T:window-status-format}')"
FMT_B="$(T display-message -p -t "$PB" '#{T:window-status-format}')"
echo "  | A 页标签: $FMT_A"; echo "  | B 页标签: $FMT_B"
check "等批准是黄点加页名" bash -c "[ \"\$1\" = '#[fg=yellow]●#[fg=colour244] sh-pa' ]" _ "$FMT_A"
check "等回复是蓝点加页名" bash -c "[ \"\$1\" = '#[fg=brightblue]●#[fg=colour244] sh-pb' ]" _ "$FMT_B"
TALLY="$("$BIN" tally)"
echo "  | tally: $TALLY"
check "tally 有 1 个等批准、2 个等回复（sh-pb 和前面的假 claude）" bash -c "[ \"\$1\" = '#[fg=yellow]等你批准 1#[default]  #[fg=brightblue]等你回复 2#[default]' ]" _ "$TALLY"
check "tmux 能展开状态栏右边" bash -c "T() { '$TMUX_BIN' -L $SHIFU_SOCKET \"\$@\"; }; T show-options -gv status-right | grep -q 'tally'"

echo "== v1.1：并排到右边"
OLD_WB="$(win_of "$PB")"
act_in "$PA" right $B
check "B 的窗格进了 A 的窗口" test "$(win_of "$PB")" = "$(win_of "$PA")"
check "B 原来的窗口没了" not has_window "$OLD_WB"
check "页名是 sh-pa + sh-pb" test "$(pane_info "$PA" '#{window_name}')" = "sh-pa + sh-pb"
check "B 在 A 的右边、同一行" same_row_right_of "$PB" "$PA"
SIDE_A="$(sidebar_of "$(win_of "$PA")")"
check "侧栏仍是 32 列" test "$(pane_info "$SIDE_A" '#{pane_width}')" = 32
FMT_J="$(T display-message -p -t "$PA" '#{T:window-status-format}')"
echo "  | 并排后的标签: $FMT_J"
check "一页两个点，页名连起来" bash -c "[ \"\$1\" = '#[fg=yellow]●#[fg=brightblue]●#[fg=colour244] sh-pa + sh-pb' ]" _ "$FMT_J"
check "已经在这一页 → 报错不动" refuses "$PA" 已经在这一页了 right $B

echo "== v1.1：平均排列"
T resize-pane -t "$PA" -x 20
act_in "$PA" even $A
WA="$(pane_info "$PA" '#{pane_width}')"; WB="$(pane_info "$PB" '#{pane_width}')"
echo "  | A 宽 $WA，B 宽 $WB"
check "两个窗格宽度差不超过 1" within_one '#{pane_width}' "$PA" "$PB"
check "平均后侧栏还是 32 列" test "$(pane_info "$SIDE_A" '#{pane_width}')" = 32

echo "== v1.1：单独成页"
act_in "$PA" own $B
check "B 单独一个窗口" test "$(win_of "$PB")" != "$(win_of "$PA")"
check "A 的页名回到 sh-pa" test "$(pane_info "$PA" '#{window_name}')" = "sh-pa"
check "B 的页名是 sh-pb" test "$(pane_info "$PB" '#{window_name}')" = "sh-pb"
check "B 的窗口左边有侧栏（2 个窗格，第 0 个是 shifu）" sidebar_shifu "$(win_of "$PB")"
check "已经是单独一页 → 报错" refuses "$PB" 已经是单独一页 own $B

echo "== v1.1：并排到下面，再关闭其中一个"
act_in "$PA" below $B
check "B 在 A 的下面" same_column_below "$PB" "$PA"
act_in "$PA" even $A
check "上下两个窗格高度差不超过 1" within_one '#{pane_height}' "$PA" "$PB"
act_in "$PA" close $B
check "close 后 B 的窗格没了" not has_pane "$PB"
check "close 后 B 的文件删了" test ! -e "$SHIFU_HOME/agents/$B.json"
check "close 后页名回到 sh-pa" test "$(pane_info "$PA" '#{window_name}')" = "sh-pa"
check "A 还活着" bash -c "$BIN ls | grep -q '^sh-pa '"
"$BIN" new sh "$TMP/pc" -- -c 'sleep 600'
PC="$(jf sh-pc pane_id)"; WC="$(win_of "$PC")"
act_in "$PA" close sh-pc
check "独占一页的 agent 关掉后整页消失" not has_window "$WC"

echo "== v1.1：jump 按 tmux 里的位置跳"
HOME_WIN="$(T list-windows -F '#{window_id} #{window_name}' | awk '$2=="home"{print $1}')"
T select-window -t "$HOME_WIN"
act_in "$PA" jump $A
check "jump 切到 A 所在的窗口" test "$(T display-message -p -t shifu: '#{window_id}')" = "$(win_of "$PA")"

echo "== v1.1：并到主页（页里只有 shell）再拆回去"
HOME_SIDE="$(sidebar_of "$HOME_WIN")"
act_in "$(T list-panes -t "$HOME_WIN" -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')" right $A
check "A 进了主页" test "$(win_of "$PA")" = "$HOME_WIN"
check "主页的页名成了 sh-pa" test "$(pane_info "$PA" '#{window_name}')" = "sh-pa"
check "A 在主页 shell 的右边" same_row_right_of "$PA" "$(T list-panes -t "$HOME_WIN" -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')"
act_in "$PA" own $A
check "拆出去后 A 不在主页" test "$(win_of "$PA")" != "$HOME_WIN"
check "主页的页名回到 home" test "$(pane_info "$HOME_SIDE" '#{window_name}')" = "home"

echo "== v1.1：恢复会话"
check "还在运行 → 拒绝恢复" refuses "$PA" 还在运行 resume $A
kill -9 "$(pane_info "$PA" '#{pane_pid}')"
wait_for "A 的进程死了，显示已退出" ls_has "^sh-pa .*已退出"
act_in "$PA" resume $A
wait_for "resume 后 A 又在运行" ls_has "^sh-pa .*运行中"
check "pane_id 没变" test "$(jf $A pane_id)" = "$PA"
check "窗格不再是死的" test "$(pane_info "$PA" '#{pane_dead}')" = 0
check "agent 文件状态回到 starting" test "$(jf $A status)" = starting
check "恢复后 @shifu_status 是 running（没有 hook 的命令）" test "$(opt "$PA")" = running

echo "== v1.1：窗格整个没了时，resume 新开一页"
mkdir -p "$TMP/pd"
"$BIN" new sh "$TMP/pd" -- -c 'sleep 600'
PD_OLD="$(jf sh-pd pane_id)"
T kill-window -t "$(win_of "$PD_OLD")"
check "窗格没了，ls 显示已退出" ls_has "^sh-pd .*已退出"
act_in "$PA" resume sh-pd
check "新开了窗格，pane_id 换了" test "$(jf sh-pd pane_id)" != "$PD_OLD"
check "新窗格在运行" ls_has "^sh-pd .*运行中"
check "新窗格的 @shifu_status 是 running（没有 hook 的命令）" test "$(opt "$(jf sh-pd pane_id)")" = running

echo "== v1.1：假 claude 带 session_id 恢复"
hook_as claude SessionStart '{"session_id":"sess-123"}'
check "session_id 记下了" test "$(jf claude session_id)" = sess-123
CPANE="$(jf claude pane_id)"
kill -9 "$(pane_info "$CPANE" '#{pane_pid}')"
wait_for "假 claude 显示已退出" ls_has "^claude .*已退出"
rm -f "$TMP/rec/claude.args"
act_in "$CPANE" resume claude
wait_for "假 claude 重新收到参数" test -f "$TMP/rec/claude.args"
check "带 --resume sess-123" bash -c "grep -qx -- '--resume' '$TMP/rec/claude.args' && grep -qx 'sess-123' '$TMP/rec/claude.args'"
check "仍然带 --settings" grep -qx -- '--settings' "$TMP/rec/claude.args"
check "claude 的 pane_id 没变" test "$(jf claude pane_id)" = "$CPANE"

echo "== v1.1：假 codex 没有 session_id 时 resume --last"
XPANE="$(jf codex pane_id)"
kill -9 "$(pane_info "$XPANE" '#{pane_pid}')"
wait_for "假 codex 显示已退出" ls_has "^codex .*已退出"
rm -f "$TMP/rec/codex.args"
act_in "$XPANE" resume codex
wait_for "假 codex 重新收到参数" test -f "$TMP/rec/codex.args"
check "第一个参数是 resume" bash -c "[ \"\$(sed -n 1p '$TMP/rec/codex.args')\" = resume ]"
check "还是七个 -c" bash -c "[ \"\$(grep -cx -- '-c' '$TMP/rec/codex.args')\" = 7 ]"
check "最后一个参数是 --last" bash -c "[ \"\$(tail -n 1 '$TMP/rec/codex.args')\" = '--last' ]"

echo "== v1.1：SessionEnd 把圆点变成 exited"
hook_as sh-2 SessionEnd
check "@shifu_status 是 exited" test "$(opt "$(jf sh-2 pane_id)")" = exited

if [ "$fail" = 0 ]; then echo "SMOKE PASS"; else echo "SMOKE FAIL"; fi
exit "$fail"
