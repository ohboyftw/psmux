<#
.SYNOPSIS
    Wire conformance check of `psmux bus` against the real Python mycel client.

.DESCRIPTION
    Starts one private `psmux bus` on a free port, runs pub/sub, replay,
    unsubscribe, status and shutdown checks with the Python client from the
    mycel repo, then stops that bus.

    Safety:
    - USERPROFILE and HOME point at a fresh temp directory for the whole run,
      so neither the bus nor the Python client reads or writes your real
      ~/.mycel/server or ~/.psmux. The script checks your real
      ~/.mycel/server is unchanged at the end.
    - The bus runs on a free port, never 4321, and PSMUX_BUS=0 is set.
    - The only process this script ever stops is the bus it started, by the
      PID it recorded, and only if a clean shutdown did not end it.

    Build first (no --release needed): cargo build
    Requires Python with the `protobuf` package, and the mycel repo's
    clients/python directory (mycel_client.py and mycel_pb2.py).

.EXAMPLE
    pwsh scripts/mycel_bus_conformance.ps1
.EXAMPLE
    pwsh scripts/mycel_bus_conformance.ps1 -Exe D:\Home\psmux\target\debug\psmux.exe
#>
[CmdletBinding()]
param(
    # psmux.exe that has the `bus` subcommand. The installed one may not.
    [string]$Exe = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'debug\psmux.exe' } else { Join-Path $PSScriptRoot '..\target\debug\psmux.exe' }),
    [string]$ClientDir = 'D:\Home\mycel\clients\python',
    [string]$Python = 'py'
)

$ErrorActionPreference = 'Stop'

$PythonChecks = @'
import json
import socket
import sys
import time

client_dir, port, bus_pid, disc = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
sys.path.insert(0, client_dir)
try:
    import mycel_pb2 as pb
    from mycel_client import MycelClient, discover_server
except ImportError as e:
    print(f"SKIP cannot import the Python mycel client: {e}")
    sys.exit(2)

failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + ("" if ok or not detail else f"  ({detail})"))
    if not ok:
        failures.append(name)


def connect(cid, host="127.0.0.1", p=None):
    c = MycelClient(host=host, port=p or port, client_id=cid)
    c.connect()
    c._sock.settimeout(5)
    return c


def frame(c):
    try:
        return c._read_envelope()
    except (socket.timeout, OSError):
        return None


def request(c, env):
    c._send(env)
    return frame(c)


def sub_raw(c, pattern):
    f = request(c, pb.Envelope(subscribe=pb.Subscribe(topic_pattern=pattern)))
    return f is not None and f.HasField("ack") and f.ack.ref == "subscribe"


def event(c):
    f = frame(c)
    return f.event if f is not None and f.HasField("event") else None


def run(name, fn):
    try:
        fn()
    except Exception as e:  # report and keep going, one failed check is not fatal
        check(name, False, f"{type(e).__name__}: {e}")


def t_discovery():
    info = json.loads(open(disc, encoding="utf-8").read())
    check("discovery file names 127.0.0.1", info.get("host") == "127.0.0.1", info)
    check("discovery file names the port", info.get("port") == port, info)
    check("discovery file carries the bus pid", info.get("pid") == bus_pid, info)
    check("python discover_server() finds the bus", discover_server() == ("127.0.0.1", port), discover_server())
    c = MycelClient(client_id="conf-disc")
    c.connect()
    c._sock.settimeout(5)
    f = request(c, pb.Envelope(status_req=pb.StatusReq()))
    check("hello via discovery, then StatusResp is the next frame", f is not None and f.HasField("status_resp"))
    c.close()


def t_pubsub():
    s = connect("conf-sub")
    check("subscribe gets Ack{subscribe} as the next frame", sub_raw(s, "conf/#"))
    p = connect("conf-pub")
    p.publish("conf/a", "hello")
    e = event(s)
    check("subscriber receives the event", e is not None and e.topic == "conf/a", e)
    if e is not None:
        check("payload is carried", e.payload == b"hello", e.payload)
        check("first event on a topic has sequence 1", e.sequence == 1, e.sequence)
        check("publisher is the Hello client_id", e.publisher == "conf-pub", e.publisher)
    s.close()
    p.close()


def t_plus():
    s = connect("conf-plus")
    sub_raw(s, "plus/+")
    p = connect("conf-pub")
    p.publish("plus/a/b", "deep")
    p.publish("plus/c", "one")
    e = event(s)
    check("'+' matches one level only", e is not None and e.topic == "plus/c", e)
    s.close()
    p.close()


def t_dollar():
    mon = connect("conf-mon")
    check("subscribe '#'", sub_raw(mon, "#"))
    check("subscribe '+/#'", sub_raw(mon, "+/#"))
    sysmon = connect("conf-sysmon")
    check("subscribe '$mycel/#'", sub_raw(sysmon, "$mycel/#"))
    newcomer = connect("conf-newcomer")
    e = event(sysmon)
    check("'$mycel/#' sees $mycel/clients/connected",
          e is not None and e.topic == "$mycel/clients/connected", e)
    p = connect("conf-pub")
    p.publish("dollar/marker", "m")
    e = event(mon)
    check("'#' and '+/#' skip $-topics (first event is the marker)",
          e is not None and e.topic == "dollar/marker", e)
    for c in (mon, sysmon, newcomer, p):
        c.close()


def t_replay():
    p = connect("conf-replay-pub")
    for i in range(3):
        p.publish("replay/x", f"r{i}")
    p.close()  # publish then close at once: events must still be stored
    time.sleep(0.5)
    r = connect("conf-replay")
    evs = r.replay("replay/#", since_minutes=5)
    check("replay returns the 3 events then replay_complete", len(evs) == 3, len(evs))
    check("replay keeps publish order", [e.payload for e in evs] == [b"r0", b"r1", b"r2"],
          [e.payload for e in evs])
    check("replay sequences are 1,2,3", [e.sequence for e in evs] == [1, 2, 3],
          [e.sequence for e in evs])
    all_evs = r.replay("#", since_minutes=5)
    check("replay '#' returns no $-topics",
          len(all_evs) > 0 and not any(e.topic.startswith("$") for e in all_evs),
          [e.topic for e in all_evs])
    sys_evs = r.replay("$mycel/#", since_minutes=5)
    check("replay '$mycel/#' returns presence events", len(sys_evs) > 0, len(sys_evs))
    r.close()


def t_replay_gap():
    r = connect("conf-gap")
    r.replay("gap/#", since_minutes=5)
    p = connect("conf-gap-pub")
    p.publish("gap/x", "between")
    time.sleep(0.3)
    check("subscribe after replay is acked", sub_raw(r, "gap/#"))
    e = event(r)
    check("event published between replay and subscribe is delivered",
          e is not None and e.payload == b"between", e)
    r.close()
    p.close()


def t_unsubscribe():
    s = connect("conf-unsub")
    sub_raw(s, "unsub/a/#")
    sub_raw(s, "unsub/b/#")
    f = request(s, pb.Envelope(unsubscribe=pb.Unsubscribe(topic_pattern="unsub/a/#")))
    check("unsubscribe gets Ack{unsubscribe}", f is not None and f.HasField("ack") and f.ack.ref == "unsubscribe", f)
    p = connect("conf-pub")
    p.publish("unsub/a/x", "gone")
    p.publish("unsub/b/x", "kept")
    e = event(s)
    check("unsubscribe removes only the named pattern", e is not None and e.topic == "unsub/b/x", e)
    s.close()
    p.close()


def t_invalid():
    s = connect("conf-invalid")
    f = request(s, pb.Envelope(subscribe=pb.Subscribe(topic_pattern="a/#/b")))
    check("subscribe 'a/#/b' is refused with Error", f is not None and f.HasField("error"), f)
    s.close()


def t_status():
    s = connect("conf-status")
    f = request(s, pb.Envelope(status_req=pb.StatusReq()))
    ok = f is not None and f.HasField("status_resp")
    check("StatusReq gets StatusResp as the next frame", ok)
    if not ok:
        return
    st = f.status_resp
    # This Python stub predates ClientInfo.events_dropped (field 7), so the
    # drop counter is skipped here; it is covered by the Rust tests.
    check("status counts events", st.total_events > 0, st.total_events)
    check("status lists published topics", any(t.topic == "conf/a" for t in st.topics),
          [t.topic for t in st.topics])
    check("status lists this client", any(c.client_id == "conf-status" for c in st.clients),
          [c.client_id for c in st.clients])
    s.close()


def t_shutdown():
    if not hasattr(pb, "Shutdown"):
        print("SKIP shutdown: this mycel_pb2 has no Shutdown message")
        return
    s = connect("conf-stop")
    f = request(s, pb.Envelope(shutdown=pb.Shutdown(reason="conformance run")))
    check("Shutdown gets Ack{shutdown}", f is not None and f.HasField("ack") and f.ack.ref == "shutdown", f)
    s.close()


for name, fn in [("discovery", t_discovery), ("pub/sub", t_pubsub), ("plus", t_plus),
                 ("dollar", t_dollar), ("replay", t_replay), ("replay gap", t_replay_gap),
                 ("unsubscribe", t_unsubscribe), ("invalid pattern", t_invalid),
                 ("status", t_status), ("shutdown", t_shutdown)]:
    run(name, fn)

print(f"{len(failures)} failure(s)")
sys.exit(1 if failures else 0)
'@

function Get-FreePort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    $listener.Stop()
    return $port
}

function Get-FileHashOrNull([string]$Path) {
    if (Test-Path -LiteralPath $Path) { return (Get-FileHash -LiteralPath $Path).Hash }
    return $null
}

$Exe = [System.IO.Path]::GetFullPath($Exe)
if (-not (Test-Path -LiteralPath $Exe)) { throw "psmux.exe not found at $Exe (run cargo build, or pass -Exe)" }
if (-not (Test-Path -LiteralPath (Join-Path $ClientDir 'mycel_client.py'))) {
    Write-Host "SKIP: no Python mycel client in $ClientDir"
    exit 0
}

# Captured before USERPROFILE changes, to prove the run never touched it.
$realDiscovery = Join-Path $env:USERPROFILE '.mycel\server'
$realHashBefore = Get-FileHashOrNull $realDiscovery

$saved = @{}
foreach ($name in 'USERPROFILE', 'HOME', 'PSMUX_BUS', 'PSMUX_BUS_PORT', 'PSMUX_BUS_DISCOVERY') {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name)
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("psmux-bus-conf-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
$discovery = Join-Path $tmp '.mycel\server'
$port = Get-FreePort
$bus = $null
$exitCode = 1

try {
    $env:USERPROFILE = $tmp
    $env:HOME = $tmp
    $env:PSMUX_BUS = '0'
    $env:PSMUX_BUS_PORT = "$port"
    $env:PSMUX_BUS_DISCOVERY = $discovery

    $bus = Start-Process -FilePath $Exe -ArgumentList @('bus', '--port', "$port", '--discovery', "`"$discovery`"") `
        -WorkingDirectory $tmp -WindowStyle Hidden -PassThru
    Write-Host "bus pid $($bus.Id) on 127.0.0.1:$port, discovery $discovery"

    $deadline = (Get-Date).AddSeconds(10)
    while (-not (Test-Path -LiteralPath $discovery) -and -not $bus.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if ($bus.HasExited) { throw "the bus exited at once (code $($bus.ExitCode))" }
    if (-not (Test-Path -LiteralPath $discovery)) { throw "the bus wrote no discovery file within 10 s" }

    $checks = Join-Path $tmp 'bus_checks.py'
    Set-Content -LiteralPath $checks -Value $PythonChecks -Encoding utf8
    & $Python -I $checks $ClientDir $port $bus.Id $discovery
    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 2) { Write-Host 'SKIP: Python client or protobuf unavailable'; $exitCode = 0 }

    if (-not $bus.WaitForExit(10000)) {
        Write-Host 'the bus is still running after Shutdown; asking it with psmux bus stop'
        & $Exe bus stop --port $port --discovery $discovery
        if (-not $bus.WaitForExit(10000)) { Write-Host 'FAIL bus did not stop'; $exitCode = 1 }
    }
    if ($bus.HasExited) {
        $gone = -not (Test-Path -LiteralPath $discovery)
        Write-Host ($(if ($gone) { 'PASS' } else { 'FAIL' }) + ' clean stop removes the discovery file')
        if (-not $gone) { $exitCode = 1 }
    }
}
finally {
    if ($bus -and -not $bus.HasExited) {
        # Last resort, and only for the process this script started.
        Stop-Process -Id $bus.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
    }
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$realHashAfter = Get-FileHashOrNull $realDiscovery
if ($realHashBefore -ne $realHashAfter) {
    Write-Host "FAIL your real $realDiscovery changed during the run"
    $exitCode = 1
} else {
    Write-Host "PASS real $realDiscovery untouched"
}
exit $exitCode
