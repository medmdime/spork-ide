#!/bin/sh
# usage: sandbox.sh <scratch dir> [project dir]
#
# Starts a second reference app, apart from the one you use: its settings, caches, plugins and logs live
# in <scratch dir>, and it carries JetBrains' Remote Robot server (the plugin IntelliJ's own UI
# tests use) on port 8082, which robot.py and capture.py talk to. The project defaults to a fresh
# copy of the end-to-end tests' fixture (tools/e2e/fixture.sh), so the reference app's screenshots and
# Spork's show the same repository. Quit it like any app when the catalogue is done.
#
# Another IntelliJ-based app: APP is the .app, LAUNCHER its Contents/MacOS program, VMOPTIONS its
# Contents/bin options file, ENV_PREFIX what its launcher's variables start with (<prefix>_PROPERTIES,
# _VM_OPTIONS, _JDK), FIXTURE the script writing the project (given the project and the sandbox's
# config folder). IntelliJ IDEA Community Edition:
#   APP="/Applications/IntelliJ IDEA CE.app" LAUNCHER=idea VMOPTIONS=idea.vmoptions ENV_PREFIX=IDEA \
#     FIXTURE=./ide-fixture.sh ./sandbox.sh <scratch>
set -eu
scratch=$(cd "$1" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
app=${APP:-${REFERENCE_APP:-/Applications/Reference.app}}
launcher=${LAUNCHER:-reference}
vmoptions=${VMOPTIONS:-reference.vmoptions}
prefix=${ENV_PREFIX:-REFERENCE}
fixture=${FIXTURE:-$here/../e2e/fixture.sh}
# 0.11.23 is the last release; it asks for build 192 or later and sets no upper bound (252, 263)
robot=0.11.23
mkdir -p "$scratch/config" "$scratch/system" "$scratch/plugins" "$scratch/log"
if [ ! -d "$scratch/plugins/robot-server-plugin" ]; then
  curl -sfL -o "$scratch/robot.zip" \
    "https://packages.jetbrains.team/maven/p/ij/intellij-dependencies/com/intellij/remoterobot/robot-server-plugin/$robot/robot-server-plugin-$robot.zip"
  unzip -q -o "$scratch/robot.zip" -d "$scratch/plugins"
fi
cat > "$scratch/idea.properties" <<EOF
idea.config.path=$scratch/config
idea.system.path=$scratch/system
idea.plugins.path=$scratch/plugins
idea.log.path=$scratch/log
EOF
# the app's own options, then the robot's port and no first-run questions
cat "$app/Contents/bin/$vmoptions" > "$scratch/$vmoptions"
cat >> "$scratch/$vmoptions" <<'EOF'
-Drobot-server.port=8082
-Djb.privacy.policy.text=<!--999.999-->
-Djb.consents.confirmation.enabled=false
-Didea.trust.all.projects=true
-Dide.show.tips.on.startup.default.value=false
-Didea.initially.ask.config=never
EOF
project=${2:-$scratch/fixture}
if [ $# -lt 2 ]; then
  APP="$app" "$fixture" "$project" "$scratch/config"
fi
# <prefix>_JDK: the bundled runtime, or the launcher would read a runtime choice from your own
# config folder
env "${prefix}_PROPERTIES=$scratch/idea.properties" "${prefix}_VM_OPTIONS=$scratch/$vmoptions" \
  "${prefix}_JDK=$app/Contents/jbr" \
  "$app/Contents/MacOS/$launcher" "$project" > "$scratch/stdout.log" 2>&1 &
echo $! > "$scratch/pid"
i=0
until curl -sf -m 2 -o /dev/null http://127.0.0.1:8082/hello; do
  i=$((i + 1))
  [ $i -lt 120 ] || { echo "The robot server didn't start; see $scratch/log/idea.log" >&2; exit 1; }
  sleep 1
done
echo "$(basename "$app" .app) is ready on port 8082 with $project (pid $(cat "$scratch/pid"))"
