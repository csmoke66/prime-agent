#!/bin/sh
# install-rust.sh — one-command installer for Beehive.
#
# Installs the launcher at $PREFIX/bin/beehive and the payload at
# $PREFIX/share/beehive/ (default prefix ~/.local). Releases come from the
# project's GitHub Releases: <base>/download/v<version>/ holds the
# beehive-<version>-<platform>.tar.gz archive and its SHA256SUMS.
#
# ~/.beehive (sessions, config) is never touched.
#
# Config (env with defaults):
#   BEEHIVE_DOWNLOAD_BASE_URL  the releases base
#                              (default: https://github.com/csmoke66/beehive/releases)
#   BEEHIVE_VERSION            pin an exact version instead of the latest release
#   BEEHIVE_RUST_PREFIX        install prefix (default: ~/.local)
#   BEEHIVE_RUST_VERBOSE       1 = the --verbose output mode

set -eu

BASE_URL="${BEEHIVE_DOWNLOAD_BASE_URL:-https://github.com/csmoke66/beehive/releases}"
BASE_URL="${BASE_URL%/}"
PREFIX="${BEEHIVE_RUST_PREFIX:-$HOME/.local}"

die() { echo "install-rust.sh: $1" >&2; exit 1; }

usage() {
  cat <<'USAGE'
install-rust.sh — install Beehive.

The launcher lands at $BEEHIVE_RUST_PREFIX/bin/beehive and the payload at
$BEEHIVE_RUST_PREFIX/share/beehive/ (default prefix ~/.local). ~/.beehive is
never touched. A running beehive daemon keeps the old build until it restarts.

Needs curl + sh (+ the network); --archive needs neither the network nor curl.

Options:
  --verbose   the detail also goes to stdout, not just fd 3 (no animation)
  --archive <path>
              install a local beehive-<version>-<platform>.tar.gz release
              archive instead of downloading the latest release
Environment:
  BEEHIVE_DOWNLOAD_BASE_URL  releases base (the GitHub Releases of csmoke66/beehive)
  BEEHIVE_VERSION            pin an exact version
  BEEHIVE_RUST_PREFIX        install prefix (~/.local by default)
  BEEHIVE_RUST_VERBOSE       1 = the --verbose output mode
USAGE
}

VERBOSE="${BEEHIVE_RUST_VERBOSE:-0}"
ARCHIVE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --archive)
      [ $# -ge 2 ] || { usage >&2; die "--archive needs the path of a release archive"; }
      ARCHIVE="$2"
      shift ;;
    --verbose|-v) VERBOSE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
  shift
done

# --- the output contract ------------------------------------------------------
# Everything a person reads goes to stderr, kept short: a title, one line
# per step that did something ("✓ Installed"), warnings with the exact
# command that fixes them, and a short ending. On a terminal one renderer
# owns the screen while the install runs: an awk loop (below) that draws
# the TUI's animated brand mark, the title, the finished steps, and the
# active step's spinner (and the download bar), rewriting only the rows
# that changed each 120ms tick; it freezes on its last frame when the
# install ends. The install itself only writes the step state to files the
# renderer reads. Captured output (CI, pipes, the TUI's /update) gets the
# same lines with no colors, no animation, and no carriage returns, and a
# failure's error stays the last stderr line (the update reports that
# tail). The extra detail rides fd 3: a wrapper captures it by opening
# fd 3, a bare run leaves it closed, and --verbose folds it onto stdout.
if [ "$VERBOSE" = 1 ]; then
  exec 3>&1
elif ( : >&3 ) 2>/dev/null; then
  :   # already open — a wrapper's capture channel; keep it (the subshell
      # probe tests the DESCRIPTOR, not a /dev/fd node: on macOS /dev/fd/3
      # exists while fd 3 is closed, and a redirect onto a closed fd under
      # set -e would abort the installer at its first progress line)
else
  exec 3>/dev/null
fi
UI_TTY=0
if [ -t 2 ] && [ "${TERM:-}" != dumb ]; then UI_TTY=1; fi
# The renderer redraws rows in place, so it stays off when --verbose
# interleaves detail lines on the same terminal.
UI_ANIM=0
if [ "$UI_TTY" = 1 ] && [ "$VERBOSE" != 1 ]; then UI_ANIM=1; fi
UI_COLOR=0
if [ "$UI_TTY" = 1 ] && [ -z "${NO_COLOR:-}" ]; then UI_COLOR=1; fi
# The Beehive palette (crates/bh-types/themes/beehive.json), 24-bit.
C_BOLD="" C_DIM="" C_ACCENT="" C_WARN="" C_RESET=""
if [ "$UI_COLOR" = 1 ]; then
  esc="$(printf '\033')"
  C_BOLD="${esc}[1m"
  C_DIM="${esc}[38;2;113;113;122m"
  C_ACCENT="${esc}[38;2;124;111;175m"
  C_WARN="${esc}[38;2;245;158;11m"
  C_RESET="${esc}[0m"
fi
SAY_DIM="" SAY_RESET=""
if [ "$VERBOSE" = 1 ] && [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  SAY_DIM="$(printf '\033')[38;2;113;113;122m" SAY_RESET="$(printf '\033')[0m"
fi
say() { printf '%s%s%s\n' "$SAY_DIM" "$*" "$SAY_RESET" >&3; }

# The program text is awk, not shell: its "$BEEHIVE_UI_DIR" expands later.
# shellcheck disable=SC2016
banner_awk='# BANNER_AWK_BEGIN
# The installer banner: the TUI first-run splash (crates/bh-tui/src/
# onboarding.rs mark_rows + draw_field), ported cell for cell so the same
# frame at the same width draws the same canvas; the parity test in that
# file runs this program (-v parity=1) against the Rust renderer.
function remeu(a, m) { a = a % m; if (a < 0) a += m; return a }
function absv(a) { return a < 0 ? -a : a }
function put(x, y, ch, tone, pri,   k) {
  if (y < 0 || y >= MH || x < 0 || x >= W) return
  k = y * W + x
  if (CP[k] > pri) return
  CC[k] = ch; CT[k] = tone; CP[k] = pri
}
function field(frame,   x, y, k, hash, cx, cy, contour, hy, si, seg, t, base, wave, ty, i, n, parts, eq) {
  for (k = 0; k < W * MH; k++) { CC[k] = " "; CT[k] = "D"; CP[k] = 0 }
  cx = int(W * 36 / 100); cy = int(MH * 54 / 100); hy = int(MH * 58 / 100)
  for (y = 0; y < MH; y++) for (x = 0; x < W; x++) {
    hash = (x * 37 + y * 53 + frame * 11 + x * y * 3) % 101
    if (hash < 3) put(x, y, "·", "D", 1)
    contour = absv(x - cx) + absv(y - cy) * 4 + int(x / 6) - frame
    if (x < int(W * 82 / 100) && remeu(contour, 24) == 12) put(x, y, ((x + y) % 5 == 0) ? "╌" : "·", "B", 2)
    if (y == hy && x % 2 == 0 && (x + frame) % 13 < 2) put(x, y, "─", ((x + frame) % 3 == 0) ? "A" : "D", 3)
    if (x >= QX0 && !(x <= QX1 && y <= MH - 1) && x % 4 == 0) {
      si = int(x / 4); seg = (y + si * 2 + int(frame / 2)) % 6
      if (y > 0 && y < MH - 1 && seg < 2) put(x, y, ((si + y) % 4 == 0) ? "┃" : "▎", "L", 4)
    }
  }
  for (t = 0; t < 3; t++) {
    base = (t == 0) ? int(MH * 30 / 100) : (t == 1) ? int(MH * 49 / 100) : int(MH * 72 / 100)
    for (x = 0; x < W; x++) {
      wave = (x * 2 + frame + t * 7) % 16
      if (wave > 7) wave = 15 - wave
      ty = base + int((wave - 3) / 2)
      if (ty < 0) ty = 0
      if ((x + frame + t * 13) % 41 == 0) put(x, ty, "◆", "W", 6)
      else if ((x + frame) % 12 == 0) put(x, ty, "•", "A", 6)
      else put(x, ty, "·", "A", 3)
    }
  }
  for (y = 0; y < MH; y++) {
    n = split(LOGO[y], parts, "|")
    for (i = 1; i <= n; i++) {
      eq = index(parts[i], "=")
      put(LI + substr(parts[i], 1, eq - 1), y, substr(parts[i], eq + 1), "T", 8)
    }
  }
}
function mark_row(y,   x, k, s, cur) {
  s = ""; cur = ""
  for (x = 0; x < W; x++) {
    k = y * W + x
    if (CT[k] != cur) { cur = CT[k]; s = s TONE[cur] }
    s = s CC[k]
  }
  return s RESET
}
function mb(b) { return sprintf("%.1f MB", b / 1000000) }
# Move the cursor to region row i (row 0 is the line the renderer started
# on); rows past the bottom are created with newlines.
function go(i) {
  if (i >= existing) {
    go(existing - 1)
    while (existing <= i) { printf "\n"; existing++; cur = existing - 1 }
    return
  }
  if (i < cur) printf "%s[%dA", ESC, cur - i
  else if (i > cur) printf "%s[%dB", ESC, i - cur
  cur = i
}
function compose(   n, line, label, detail, ppath, total, got, cmd, filled, bar, c) {
  n = 0
  if (banner == 1) {
    field(frame)
    R[n++] = ""
    for (y = 0; y < MH; y++) R[n++] = mark_row(y)
    R[n++] = ""
  }
  line = ""
  if ((getline line < brandf) < 0) line = ""
  close(brandf)
  R[n++] = " " TX "Installing " BOLD "BEEHIVE" RESET (line == "" ? "" : "  " DIM line RESET)
  R[n++] = ""
  while ((getline line < logf) > 0) R[n++] = line
  close(logf)
  label = ""; detail = ""; ppath = ""; total = ""
  if ((getline label < activef) > 0) {
    getline detail < activef; getline ppath < activef; getline total < activef
  }
  close(activef)
  if (label != "") {
    if (ppath != "") {
      # The path never enters the command text: the shell expands the
      # environment variable itself (a TMPDIR with shell syntax stays data).
      got = 0; cmd = "{ wc -c < \"$BEEHIVE_UI_DIR/ui-progress\"; } 2>/dev/null"
      cmd | getline got; close(cmd); got += 0
      if (total + 0 > 0) {
        filled = int(got * 20 / total); if (filled > 20) filled = 20
        bar = ""
        for (c = 0; c < 20; c++) bar = bar (c < filled ? "█" : "░")
        detail = AC bar RESET DIM "  " mb(got) " / " mb(total) (detail == "" ? "" : "  · " detail)
      } else detail = mb(got) (detail == "" ? "" : "  · " detail)
    }
    R[n++] = " " AC SPIN[frame % 10 + 1] RESET " " label (detail == "" ? "" : "  " DIM detail RESET)
  }
  return n
}
function draw(n,   i) {
  for (i = 0; i < n; i++) {
    if ((i in PREV) && PREV[i] == R[i]) continue
    PREV[i] = R[i]
    # A row scrolled off the top can no longer be reached: leave it.
    if (i < existing - TH) continue
    go(i); printf "\r%s[2K%s", ESC, R[i]
  }
  for (i = n; i < pn; i++) { if (i >= existing - TH) { go(i); printf "\r%s[2K", ESC }; delete PREV[i] }
  pn = n
  fflush()
}
BEGIN {
  ESC = "\033"
  MH = 7; LI = 5; QX0 = 5; QX1 = 26
  LOGO[0] = "8=▀|9=▀|10=▀|11=▀|12=▀|13=▀"
  LOGO[1] = "5=▀|6=▀|7=▀|8=▀|9=▀|10=▀|11=▀|12=▀|13=▀|14=▀|15=▀|16=▀"
  LOGO[2] = "3=▀|4=▀|5=▀|6=▀|7=▀|8=▀|9=▀|10=▀|11=▀|12=▀|13=▀|14=▀|15=▀|16=▀|17=▀|18=▀"
  LOGO[3] = "2=▀|3=▀|4=▀|5=▀|6=▀|7=▀|8=▀|9=▀|10=▀|11=▀|12=▀|13=▀|14=▀|15=▀|16=▀|17=▀|18=▀|19=▀"
  LOGO[4] = "1=▀|2=▀|3=▀|4=▀|5=▀|6=▀|7=▀|8=▀|9=▀|10=▀|11=▀|12=▀|13=▀|14=▀|15=▀|16=▀|17=▀|18=▀|19=▀|20=▀"
  LOGO[5] = "1=▀|2=▀|3=▀|4=▀|5=▀|6=▀|7=▀|8=▀|9=▀|12=▀|13=▀|14=▀|15=▀|16=▀|17=▀|18=▀|19=▀|20=▀"
  LOGO[6] = "0=▀|1=▀|2=▀|3=▀|4=▀|5=▀|6=▀|7=▀|8=▀|13=▀|14=▀|15=▀|16=▀|17=▀|18=▀|19=▀|20=▀|21=▀"
  if (color == 1) {
    RESET = ESC "[0m"; BOLD = ESC "[1m"; ITAL = ESC "[3m"; TX = ESC "[39m"
    DIM = ESC "[38;2;113;113;122m"; AC = ESC "[38;2;124;111;175m"
    TONE["T"] = ESC "[39m"; TONE["D"] = DIM; TONE["B"] = ESC "[38;2;82;82;91m"
    TONE["A"] = AC; TONE["L"] = ESC "[38;2;56;189;248m"; TONE["W"] = ESC "[38;2;245;158;11m"
  }
  W = width + 0
  if (parity == 1) {
    field(frame + 0)
    for (y = 0; y < MH; y++) {
      chars = ""; tones = ""
      for (x = 0; x < W; x++) { chars = chars CC[y * W + x]; tones = tones CT[y * W + x] }
      print chars "\t" tones
    }
    exit
  }
  split("⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏", SPIN, " ")
  uidir = ENVIRON["BEEHIVE_UI_DIR"]
  activef = uidir "/ui-active"; logf = uidir "/ui-log"; brandf = uidir "/ui-brand"; stopf = uidir "/ui-stop"
  TH = height + 0; if (TH < 5) TH = 24
  frame = 0; existing = 1; cur = 0; pn = 0
  printf "%s[?25l%s[?7l", ESC, ESC
  while (1) {
    stopping = ((getline tmp < stopf) >= 0); close(stopf)
    n = compose()
    draw(n)
    if (stopping) break
    if (system("kill -0 " ppid " 2>/dev/null && { sleep 0.12 2>/dev/null || sleep 1; }") != 0) break
    frame++
  }
  go(n - 1)
  printf "\n%s[?7h%s[?25h", ESC, ESC
  fflush()
}
# BANNER_AWK_END
'

dl="$(mktemp -d "${TMPDIR:-/tmp}/beehive-download.XXXXXX")"
# The renderer's inputs: the active step (label, detail, the file being
# downloaded, its size), the finished lines, the title's version text, and
# the stop flag. Warnings wait in ui_notes and print in full once the
# renderer has stopped (a frozen frame never clips them).
ui_active="${dl}/ui-active"
ui_log="${dl}/ui-log"
ui_brand="${dl}/ui-brand"
ui_stopf="${dl}/ui-stop"
ui_notes="${dl}/ui-notes"
: > "$ui_log"
: > "$ui_brand"
: > "$ui_notes"
ui_pid=""
ui_step=""
ui_progress_path=""
ui_progress_total=""
ui_size() {
  stty size < /dev/tty 2>/dev/null || true
}
if [ "$UI_ANIM" = 1 ]; then
  ui_dims="$(ui_size)"
  ui_rows="${ui_dims%% *}"
  ui_cols="${ui_dims##* }"
  case "$ui_cols" in ""|0|*[!0-9]*) ui_cols="${COLUMNS:-80}" ;; esac
  case "$ui_rows" in ""|0|*[!0-9]*) ui_rows="${LINES:-24}" ;; esac
  case "$ui_cols" in ""|*[!0-9]*) ui_cols=80 ;; esac
  # The mark needs its indent + width (27 columns); the canvas spans the
  # terminal (one column short of the edge, so a full row never wraps),
  # capped at 120 columns.
  ui_width=$((ui_cols - 1))
  [ "$ui_width" -le 120 ] || ui_width=120
  ui_banner=0
  if [ "$UI_COLOR" = 1 ] && [ "$ui_width" -ge 40 ]; then ui_banner=1; fi
  # The paths ride the environment, never -v (which rewrites backslashes)
  # and never a command string.
  BEEHIVE_UI_DIR="$dl" awk -v banner="$ui_banner" -v color="$UI_COLOR" \
    -v width="$ui_width" -v height="$ui_rows" -v ppid="$$" "$banner_awk" >&2 &
  ui_pid=$!
fi
# ui_stop: freeze the renderer on its last frame, then print the waiting
# warnings in full.
ui_stop() {
  if [ -n "$ui_pid" ]; then
    rm -f "$ui_active"
    : > "$ui_stopf"
    wait "$ui_pid" 2>/dev/null || true
    ui_pid=""
    if [ -s "$ui_notes" ]; then
      printf '\n' >&2
      cat "$ui_notes" >&2
      : > "$ui_notes"
    fi
  fi
}
# ui_set_active <label> [detail]: the renderer's active row (written whole,
# then renamed, so a tick never reads half a file).
ui_set_active() {
  printf '%s\n%s\n%s\n%s\n' "$1" "${2:-}" "$ui_progress_path" "$ui_progress_total" > "${ui_active}.tmp"
  mv -f "${ui_active}.tmp" "$ui_active"
}
# A compact size: 19855079 -> "19.9 MB".
ui_mb() {
  ui_tenths=$((($1 + 50000) / 100000))
  printf '%d.%d MB' "$((ui_tenths / 10))" "$((ui_tenths % 10))"
}
# ui_line <text>: one finished line (the renderer's list, or stderr).
ui_line() {
  if [ -n "$ui_pid" ]; then
    printf '%s\n' "$1" >> "$ui_log"
  else
    printf '%s\n' "$1" >&2
  fi
}
# title <version text>: the title line's version (the renderer shows the
# brand line from the start; captured output prints it once here).
title() {
  if [ -n "$ui_pid" ]; then
    printf '%s\n' "$1" > "$ui_brand"
  else
    printf ' %sInstalling Beehive%s  %s%s%s\n\n' "$C_BOLD" "$C_RESET" "$C_DIM" "$1" "$C_RESET" >&2
  fi
}
# step_start <label>: the step is running (the spinner row on a terminal;
# nothing in captured output, where only the result line prints).
step_start() {
  ui_step="$1"
  if [ -n "$ui_pid" ]; then ui_set_active "$1"; fi
}
# step_detail <text>: the active step's current detail (the spinner row on
# a terminal; an indented line in captured output).
step_detail() {
  if [ -n "$ui_pid" ]; then
    ui_set_active "$ui_step" "$1"
  else
    printf '   %s\n' "$1" >&2
  fi
}
step_end() {
  ui_step=""
  ui_progress_path=""
  ui_progress_total=""
  if [ -n "$ui_pid" ]; then rm -f "$ui_active"; fi
}
# step_ok <text> [detail]: the step's result line.
step_ok() {
  step_end
  ui_line " ${C_ACCENT}✓${C_RESET} $1${2:+  ${C_DIM}($2)${C_RESET}}"
}
# step_fail <text> [reason]: a failed step's result line.
step_fail() {
  step_end
  ui_line " ${C_WARN}✗ $1${2:+  ($2)}${C_RESET}"
}
# step_clear: the step finished with nothing worth a line.
step_clear() {
  step_end
}
# note: a warning, in full, in amber; todo: the exact command that fixes
# it, uncolored so it copies cleanly. On a terminal both wait for the
# renderer to stop.
note() {
  if [ -n "$ui_pid" ]; then
    printf '%s%s%s\n' "$C_WARN" "$*" "$C_RESET" >> "$ui_notes"
  else
    printf '%s%s%s\n' "$C_WARN" "$*" "$C_RESET" >&2
  fi
}
todo() {
  if [ -n "$ui_pid" ]; then
    printf '    %s\n' "$*" >> "$ui_notes"
  else
    printf '    %s\n' "$*" >&2
  fi
}
dim() {
  printf ' %s%s%s\n' "$C_DIM" "$*" "$C_RESET" >&2
}
die() {
  if [ -n "$ui_step" ]; then step_fail "$ui_step"; fi
  ui_stop
  printf '%sinstall-rust.sh: %s%s\n' "$C_WARN" "$1" "$C_RESET" >&2
  exit 1
}
# A path with the home directory spelled as ~ (display only).
tilde() {
  case "$1" in
    "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
    *) printf '%s' "$1" ;;
  esac
}
trap 'ui_stop; rm -rf "$dl" "${stage:-}"' EXIT
trap 'ui_stop; exit 130' INT
trap 'ui_stop; exit 143' HUP TERM
stage=""

# Every network read goes through fetch: a connect timeout, a whole-call
# cap, a stall cutoff (under 1 KB/s for 30s), and 3 attempts — a stalled
# connection fails with the URL named instead of hanging forever. The loop
# replaces curl's --retry-all-errors, which older curls (macOS) lack; a
# retry resumes the partial file (-C -) instead of starting over. The last
# curl error lands in fetch_error for the caller's message.
# Usage: fetch <url> <output file> <max seconds> [bar]
fetch() {
  fetch_attempt=1
  fetch_error=""
  rm -f "$2"
  if [ "${4:-}" = bar ] && [ -n "$ui_pid" ]; then
    ui_progress_total="$(curl -fsSIL --connect-timeout 10 --max-time 15 "$1" 2>/dev/null \
      | tr -d '\r' | awk 'tolower($1) == "content-length:" { size = $2 } END { print size }')"
    case "$ui_progress_total" in ""|*[!0-9]*) ui_progress_total="" ;; esac
    # The renderer reads the size through a fixed link in its own dir. Git
    # Bash's default ln -s deep-copies and refuses a missing target, so
    # there the step keeps its spinner without the bar.
    if ln -sf "$2" "${dl}/ui-progress" 2>/dev/null; then
      ui_progress_path="1"
    fi
    ui_set_active "$ui_step"
  fi
  while :; do
    if curl -fsSL -C - --connect-timeout 10 --max-time "$3" \
         --speed-limit 1024 --speed-time 30 "$1" -o "$2" 2>"$2.err"; then
      rm -f "$2.err"
      return 0
    fi
    fetch_error="$(sed -n 's/^curl: ([0-9]*) //p' "$2.err" | tail -n 1)"
    [ "$fetch_attempt" -lt 3 ] || return 1
    fetch_attempt=$((fetch_attempt + 1))
    if [ "${4:-}" = bar ]; then
      step_detail "retrying (${fetch_attempt} of 3)${fetch_error:+: $fetch_error}"
    fi
    sleep 1
  done
}

# --- platform detection ------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
WINDOWS="no"
case "$OS:$ARCH" in
  Darwin:arm64) PLATFORM=darwin-arm64 ;;
  Darwin:x86_64) PLATFORM=darwin-x64 ;;
  Linux:x86_64) PLATFORM=linux-x64 ;;
  Linux:aarch64) PLATFORM=linux-arm64 ;;
  MINGW64_NT*:x86_64|MINGW_NT*:x86_64|MSYS_NT*:x86_64|CYGWIN_NT*:x86_64)
    PLATFORM=win32-x64; WINDOWS=yes ;;
  *) die "no build is published for ${OS} ${ARCH}" ;;
esac
if [ "$WINDOWS" = "yes" ]; then BINARY_NAME="beehive.exe"; else BINARY_NAME="beehive"; fi
if [ "$OS" = "Linux" ]; then
  glibc="$(ldd --version 2>&1 | head -n 1)"
  case "$glibc" in
    *musl*) die "musl libc is not supported: the Linux builds need glibc >= 2.35" ;;
  esac
  glibc="${glibc##* }"
  glibc_minor="$(printf '%s' "${glibc#*.}" | sed 's/\..*//' | tr -cd '0-9')"
  case "$glibc" in
    2.*) [ -n "$glibc_minor" ] && [ "$glibc_minor" -ge 35 ] || die "glibc ${glibc} is below the supported floor 2.35" ;;
    [0-9]*.*) ;;
    *) die "could not determine the glibc version; the Linux builds need glibc >= 2.35" ;;
  esac
fi

# --- layout ------------------------------------------------------------------
bin_dir="${PREFIX}/bin"
share_dir="${PREFIX}/share/beehive"
launcher="${bin_dir}/beehive"
mkdir -p "$bin_dir" "${PREFIX}/share"
case "$share_dir" in "$HOME/.beehive"|"$HOME/.beehive"/*) die "the prefix ${PREFIX} falls under ~/.beehive" ;; esac

# --- the release archive -----------------------------------------------------
if [ -n "$ARCHIVE" ]; then
  [ -f "$ARCHIVE" ] || die "no such archive: ${ARCHIVE}"
  asset="$ARCHIVE"
  asset_name="$(basename "$ARCHIVE")"
  VERSION="${asset_name#beehive-}"
  VERSION="${VERSION%-"${PLATFORM}".tar.gz}"
  [ "beehive-${VERSION}-${PLATFORM}.tar.gz" = "$asset_name" ] \
    || die "the archive name ${asset_name} is not beehive-<version>-${PLATFORM}.tar.gz"
  title "${VERSION}"
else
  title ""
  if [ -n "${BEEHIVE_VERSION:-}" ]; then
    VERSION="${BEEHIVE_VERSION#v}"
  else
    step_start "Finding the latest release"
    latest_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' --connect-timeout 10 --max-time 30 "${BASE_URL}/latest" || true)"
    VERSION="${latest_url##*/tag/v}"
    case "$VERSION" in ""|*/*) die "could not resolve the latest release from ${BASE_URL}/latest" ;; esac
    step_ok "Latest release" "$VERSION"
  fi
  title "${VERSION}"
  asset_name="beehive-${VERSION}-${PLATFORM}.tar.gz"
  release_url="${BASE_URL}/download/v${VERSION}"
  asset="$dl/$asset_name"
  step_start "Downloading ${asset_name}"
  fetch "${release_url}/${asset_name}" "$asset" 600 bar \
    || die "could not download ${release_url}/${asset_name}${fetch_error:+ ($fetch_error)}"
  fetch "${release_url}/SHA256SUMS" "$dl/SHA256SUMS" 30 \
    || die "could not download ${release_url}/SHA256SUMS${fetch_error:+ ($fetch_error)}"
  step_ok "Downloaded" "$asset_name"
  step_start "Verifying the checksum"
  line="$(grep "  ${asset_name}\$" "$dl/SHA256SUMS" || true)"
  [ -n "$line" ] || die "SHA256SUMS has no line for ${asset_name}"
  printf '%s\n' "$line" > "$dl/SHA256SUMS.check"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$dl" && sha256sum -c SHA256SUMS.check >&3 2>&1) || die "checksum mismatch for ${asset_name}"
  else
    (cd "$dl" && shasum -a 256 -c SHA256SUMS.check >&3 2>&1) || die "checksum mismatch for ${asset_name}"
  fi
  step_ok "Checksum verified"
fi

# --- stage, probe, publish ---------------------------------------------------
step_start "Installing"
stage="$(mktemp -d "${PREFIX}/share/beehive.stage.XXXXXX")"
tar -xzf "$asset" -C "$stage" || { rm -rf "$stage"; die "could not extract ${asset}"; }
[ -x "${stage}/${BINARY_NAME}" ] || { rm -rf "$stage"; die "the archive did not contain an executable ${BINARY_NAME}"; }
reported_version="$("${stage}/${BINARY_NAME}" --version 2>/dev/null | head -n 1 | tr -d '\r' || true)"
[ "$reported_version" = "$VERSION" ] \
  || { rm -rf "$stage"; die "the archive names ${VERSION} but its payload reports '${reported_version}'"; }

aside=""
if [ -e "$share_dir" ] || [ -L "$share_dir" ]; then
  aside="${PREFIX}/share/beehive.old.$$"
  mv "$share_dir" "$aside" || { rm -rf "$stage"; die "could not move the current install aside"; }
fi
if ! mv "$stage" "$share_dir"; then
  [ -z "$aside" ] || mv "$aside" "$share_dir" || true
  rm -rf "$stage"
  die "could not publish the new payload"
fi
stage=""

# --- the launcher ------------------------------------------------------------
# Written literally (quoted heredoc): nothing expands at install time; the
# payload path resolves from the launcher's own location at launch.
launcher_tmp="$(mktemp "${bin_dir}/.beehive.XXXXXX")"
if [ "$WINDOWS" = "yes" ]; then
  cat > "$launcher_tmp" <<'EOF'
#!/bin/sh
# beehive — launcher written by install-rust.sh.
export BEEHIVE_CODING_AGENT_DIR="${BEEHIVE_CODING_AGENT_DIR:-$HOME/.beehive}"
exec "$(dirname "$0")/../share/beehive/beehive.exe" "$@"
EOF
else
  cat > "$launcher_tmp" <<'EOF'
#!/bin/sh
# beehive — launcher written by install-rust.sh.
export BEEHIVE_CODING_AGENT_DIR="${BEEHIVE_CODING_AGENT_DIR:-$HOME/.beehive}"
# A too-deep TMPDIR would exceed the non-Linux unix socket path limit
# (103 bytes), so that case pins /tmp.
if [ -z "${BEEHIVE_DAEMON_SOCKET:-}" ]; then
  BEEHIVE_DAEMON_SOCKET="${TMPDIR:-/tmp}/beehive-$(id -u)/daemon.sock"
  if [ "$(printf '%s' "$BEEHIVE_DAEMON_SOCKET" | wc -c)" -gt 103 ] && [ "$(uname -s)" != "Linux" ]; then
    BEEHIVE_DAEMON_SOCKET="/tmp/beehive-$(id -u)/daemon.sock"
  fi
  export BEEHIVE_DAEMON_SOCKET
fi
exec "$(dirname "$0")/../share/beehive/beehive" "$@"
EOF
fi
chmod 0755 "$launcher_tmp"
mv -f "$launcher_tmp" "$launcher"
if [ "$WINDOWS" = "yes" ]; then
  cmd_tmp="$(mktemp "${bin_dir}/.beehive-cmd.XXXXXX")"
  cat > "$cmd_tmp" <<'EOF'
@echo off
rem beehive - launcher written by install-rust.sh.
if not defined BEEHIVE_CODING_AGENT_DIR set "BEEHIVE_CODING_AGENT_DIR=%USERPROFILE%\.beehive"
"%~dp0..\share\beehive\beehive.exe" %*
EOF
  mv -f "$cmd_tmp" "${bin_dir}/beehive.cmd"
fi
[ -z "$aside" ] || rm -rf "$aside"
step_ok "Installed" "$(tilde "$launcher")"

ui_stop
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) note "${bin_dir} is not on your PATH; add it to your shell profile:"
     todo "export PATH=\"${bin_dir}:\$PATH\"" ;;
esac
ui_stop
