#!/usr/bin/env bash
# Exercise the action's own logic against a stub `gh`.
#
# The action wraps a *released* binary, so it cannot be tested by building the
# current branch. What can be tested is the part that actually breaks: mapping
# exit code 2 to `changed` rather than a failed job, the outputs, the job
# summary, and the permission annotation.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

python3 - "$root/action.yml" <<'PY'
import pathlib, sys, yaml
doc = yaml.safe_load(pathlib.Path(sys.argv[1]).read_text())
step = next(s for s in doc["runs"]["steps"] if s.get("id") == "run")
pathlib.Path("run.sh").write_text("#!/usr/bin/env bash\n" + step["run"])
PY

mkdir -p bin
cat > bin/gh <<'STUB'
#!/usr/bin/env bash
cat "$FAKE_DIR/$FAKE_CASE.json"
exit "$FAKE_STATUS"
STUB
chmod +x bin/gh

cat > drift.json <<'J'
{"version":1,"repository":"o/r","counts":{"create":2,"update":1,"delete":0,"recreate":0},
 "changes":[{"resource":"labels","op":"create","key":"bug","summary":"create label bug"}]}
J
cat > clean.json <<'J'
{"version":1,"repository":"o/r","counts":{"create":0,"update":0,"delete":0,"recreate":0},"changes":[]}
J
cat > applied.json <<'J'
{"success":true,"applied":{"create":1,"update":0,"delete":0,"recreate":0},"skipped":0,"failures":[]}
J
cat > denied.json <<'J'
{"success":false,"applied":{"create":0,"update":0,"delete":0,"recreate":0},"skipped":1,
 "failures":[{"resource":"repository","key":"settings","error":"HTTP 403","status":403}]}
J

run() {
  : > out; : > summary
  set +e
  FAKE_DIR="$work" FAKE_CASE="$1" FAKE_STATUS="$2" PATH="$work/bin:$PATH" \
    GITHUB_OUTPUT="$work/out" GITHUB_STEP_SUMMARY="$work/summary" \
    COMMAND="$3" REPOSITORY=o/r CONFIG='' ONLY='' PRUNE='' DRY_RUN=false VERBOSE=false SUMMARY=true \
    bash run.sh > stdout 2>&1
  actual=$?
  set -e
}

expect() {
  local what="$1" want="$2" got="$3"
  if [ "$got" != "$want" ]; then
    echo "FAIL: $what: expected '$want', got '$got'"
    cat stdout
    exit 1
  fi
  echo "  ok: $what = $want"
}

echo "plan with drift: exit 2 must become changed=true and a successful job"
run drift 2 plan
expect "exit code" 0 "$actual"
expect "changed" true "$(grep '^changed=' out | cut -d= -f2)"
grep -q 'create label bug' summary || { echo "FAIL: plan missing from job summary"; exit 1; }
echo "  ok: job summary lists the change"

echo "plan with no drift"
run clean 0 plan
expect "exit code" 0 "$actual"
expect "changed" false "$(grep '^changed=' out | cut -d= -f2)"
grep -q 'Nothing to do' summary || { echo "FAIL: summary should say nothing to do"; exit 1; }
echo "  ok: job summary says nothing to do"

echo "sync that applied something (sync never exits 2, so this comes from the counts)"
run applied 0 sync
expect "exit code" 0 "$actual"
expect "changed" true "$(grep '^changed=' out | cut -d= -f2)"
expect "success" true "$(grep '^success=' out | cut -d= -f2)"

echo "sync refused with 403"
run denied 1 sync
expect "exit code" 1 "$actual"
expect "success" false "$(grep '^success=' out | cut -d= -f2)"
grep -q '::error title=gh-settings::' stdout || { echo "FAIL: no permission annotation"; exit 1; }
grep -q 'cannot manage repository settings' stdout || { echo "FAIL: annotation does not explain the token"; exit 1; }
echo "  ok: annotation names the token as the likely cause"

echo
echo "All action checks passed."
