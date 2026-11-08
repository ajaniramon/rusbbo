#!/usr/bin/env bash
# Runs the Java Havana-Web next to the Java game oracle, as the reference for web traces.
#
#   web.sh build   build Havana-Web.jar into oracle/run
#   web.sh start   start the web server (if not running) on 127.0.0.1:12380
#   web.sh stop    stop it
#   web.sh reset   oracle.sh reset (database snapshot + game server), then restart the web server
#
# It uses the oracle's database (havana) and the game oracle's RCON port (12309), and serves the
# templates and static files of the Java checkout ($HAVANA_DIR/tools/www-tpl, tools/www). Like the
# game oracle, wrap every use in `flock ~/oracle.lock`.
set -euo pipefail

ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="${HAVANA_DIR:-$(cd "$ORACLE_DIR/../.." && pwd)/aeyra-havana}"
RUN_DIR="$ORACLE_DIR/run/web"
DB_PORT="${ORACLE_DB_PORT:-13307}"
WEB_PORT=12380
PID_FILE="$RUN_DIR/web.pid"

if [[ -n "${ORACLE_JAVA_HOME:-}" ]]; then
  JAVA_HOME="$ORACLE_JAVA_HOME"
elif [[ -x /opt/jdk25/bin/java ]]; then
  JAVA_HOME=/opt/jdk25
fi
export JAVA_HOME

log() { echo "[web-oracle] $*" >&2; }

running() { [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; }

build() {
  log "building Havana-Web jar"
  (cd "$REPO_DIR" && ./gradlew -q --console=plain -Porg.gradle.java.installations.paths="$JAVA_HOME" \
     :Havana-Web:shadowJar -x javadoc -x javadocJar -x sourcesJar)
  mkdir -p "$RUN_DIR"
  cp "$REPO_DIR/Havana-Web/build/libs/Havana-Web-unspecified.jar" "$RUN_DIR/Havana-Web.jar"
}

write_config() {
  mkdir -p "$RUN_DIR"
  cat > "$RUN_DIR/webserver-config.ini" <<EOF
[Site]
site.directory=$REPO_DIR/tools/www

[Global]
bind.ip=127.0.0.1
bind.port=$WEB_PORT

[Rcon]
rcon.ip=127.0.0.1
rcon.port=12309

[Database]
mysql.hostname=127.0.0.1
mysql.port=$DB_PORT
mysql.username=havana
mysql.password=goldfish
mysql.database=havana

[Template]
template.directory=$REPO_DIR/tools/www-tpl
template.name=default
template.locale.file=locale-en.ini

page.encoding=utf-8
EOF
  cp "$REPO_DIR/tools/figuredata.xml" "$RUN_DIR/"
  [[ -f "$ORACLE_DIR/run/logback.xml" ]] && cp "$ORACLE_DIR/run/logback.xml" "$RUN_DIR/" || true
}

start() {
  if running; then return; fi
  [[ -f "$RUN_DIR/Havana-Web.jar" ]] || build
  write_config
  log "starting Java web server"
  pushd "$RUN_DIR" >/dev/null
  rm -rf sessions
  nohup "$JAVA_HOME/bin/java" -Djava.net.preferIPv4Stack=true -jar Havana-Web.jar \
    > "$RUN_DIR/stdout.log" 2>&1 < /dev/null &
  echo $! > "$PID_FILE"
  popd >/dev/null
  for ((i = 0; i < 120; i++)); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$WEB_PORT") 2>/dev/null; then
      # The watchdog reads the online count and the room/group lists every 30 s from its first
      # tick; give it that first tick.
      sleep 1.5
      return 0
    fi
    sleep 0.25
  done
  log "port $WEB_PORT did not open"; return 1
}

stop() {
  if running; then
    local pid; pid=$(cat "$PID_FILE")
    kill "$pid" 2>/dev/null || true
    for ((i = 0; i < 40; i++)); do kill -0 "$pid" 2>/dev/null || break; sleep 0.25; done
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$PID_FILE"
}

case "${1:-}" in
  build) build ;;
  start) start ;;
  stop) stop ;;
  reset)
    stop
    "$ORACLE_DIR/oracle.sh" reset
    start
    ;;
  *) sed -n '2,12p' "$0"; exit 1 ;;
esac
