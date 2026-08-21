# budskit

Per-component battery and reconnect tooling for CMF / Nothing earbuds, developed
against a CMF Buds 2 (`3C:B0:ED:D1:A4:AB`).

- **`bin/cmf-budsd`** — daemon that owns the buds' single vendor RFCOMM link and
  serves their state to any number of consumers over a Unix socket. Read-only.
- **`bin/cmf-buds`** — battery for the left bud, the right bud and the case
  separately, instead of the single aggregated number BlueZ reports. Also
  firmware, serials, ANC mode, placement state and the connected host. Talks to
  the daemon when it is running, and to the device directly when it is not.
- **`bin/bt-reconnect`** — reconnect that copes with the specific ways this
  device fails, and says which one happened.
- **`bin/sdp-services`** — lists a device's RFCOMM/L2CAP services. This is how
  the vendor channel was found in the first place, and it works on any device.
- **`bin/cmfbuds.py`** — the protocol, shared by the CLI and the daemon.

The Quickshell bar widget that consumes this lives in a different repo, in
`~/dotfiles/dot-config/quickshell` — it talks to the daemon over the socket
contract documented below, so the two version independently.

`PROTOCOL.md` documents the vendor protocol these speak.

## Why there is a daemon

**The buds accept exactly one connection on their vendor channel**, and take
seconds to release it. So a CLI call, a status-bar poll and a desktop widget
cannot each open their own — whoever is second gets `EBUSY`. Everything therefore
goes through `cmf-budsd`, which holds the one link and fans state out:

```
        buds ──RFCOMM ch 16── cmf-budsd ──$XDG_RUNTIME_DIR/cmf-buds.sock──┬── cmf-buds
                              (one link)         (JSON lines)             ├── Quickshell
                                                                         └── anything else
```

Without the daemon running, `cmf-buds` opens the link itself and behaves exactly
as it did before — so nothing depends on it being up.

## Requirements

Everything needed is already installed on this box:

- BlueZ with `busctl` (systemd) — no `bluez-tools`, no `sdptool`, no root. `busctl`
  is not vendored in the nix package: it comes from the host's systemd, which any
  systemd machine has.
- Python 3 **built with Bluetooth support**. Nothing outside the stdlib is used —
  no PyBluez.

  The checked-in shebang is `/usr/bin/python3` rather than `python3` on purpose: a
  mise/pyenv interpreter is typically built without `socket.AF_BLUETOOTH` and
  cannot open a Bluetooth socket at all. The nix package rewrites the shebang to
  its own `python3`, which does have it.

### Why Python, when the house rule says Rust

`Agents.md` prefers Rust or Go. This is Python because the whole job is raw
`AF_BLUETOOTH` sockets — RFCOMM to the buds and L2CAP for the SDP query — and
CPython has both in its standard library with zero dependencies. A Rust port is
tractable if this outlives being a scratch tool: `PROTOCOL.md` pins the wire
format, and the sockets are plain `socket(AF_BLUETOOTH, …)` libc calls, so no
Bluetooth crate is strictly needed. It would buy nothing user-visible today.

## cmf-buds

```console
$ cmf-buds
Left   ██████████  95%
Right  █████████░  90%
Case   ███░░░░░░░  30%
```

Left bud in an ear, right bud in the case:

```console
$ cmf-buds info
CMF/Nothing earbuds  3C:B0:ED:D1:A4:AB   (via daemon)
  firmware        1.0.1.52   (protocol 4.0)
  ANC mode        off

  Left        100%  |  online, in-case  |  fw 1.0.1.52  |  sn 1351982513002315
  Right       100%  |  online, in-case  |  fw 1.0.1.52  |  sn 1351982513002315
  Case         20%  |  reporting

  connected host  90:A9:F7:5E:04:B5  T30Pro
```

`(via daemon)` vs `(via device)` tells you which path served the answer. Add
`--no-daemon` to force the direct link.

Once the case has reported at least once, its row shows up with the age of the
reading: `Case   ██░░░░░░░░  15%  (last seen 12m ago)`.

| Command | What it does |
|---|---|
| `cmf-buds` / `cmf-buds battery` | per-component battery |
| `cmf-buds --json` | same, machine readable, with `live` and `age` fields |
| `cmf-buds info` | firmware, serials, ANC, placement, connected host |
| `cmf-buds watch` | follow state changes as they happen |
| `cmf-buds waybar` | one-line JSON for a waybar custom module |
| `cmf-buds raw c007 [hex]` | send an arbitrary command, dump the reply |

`-m/--mac` overrides the address, as does `$CMF_BUDS_MAC`.

### Two behaviours that look like bugs but are not

**The case usually reads "not reporting".** The case only reports while at least
one bud is docked in it — you can see this in the placement frames, where its slot
appears exactly when a bud reads `in-case`. No request forces it otherwise.
`cmf-buds` therefore caches the last value per component under
`~/.cache/cmf-buds/` and shows it with its age, so a `Case` row is a real earlier
reading rather than a live one. Anything marked `live: true` in `--json` came off
the wire just now.

The one time the case did report here it read **15 %**, so it is worth charging.
That reading predates the tool, so it is not in the cache — dock a bud with
`cmf-buds watch` running and it will land there.

**Two runs back to back can fail with `EBUSY`.** The buds accept one connection
on this channel at a time and take a few seconds to release the previous one.
With `cmf-budsd` running this cannot happen at all, since nothing but the daemon
opens the link. Against the device directly, `cmf-buds` retries four times with a
backoff, which covers it.

### watch

```console
$ cmf-buds watch
listening on 3C:B0:ED:D1:A4:AB ch 16, polling every 3s - Ctrl-C to stop
10:39:29  placement  left=[online, in-ear =0x84]  right=[online, in-ear =0x84]   (poll)
10:39:29  battery    left=95%  right=90%   (poll)
10:39:29  anc        transparency   (poll)
```

It prints a line only when the decoded state actually changes, so it stays silent
while nothing happens. Each line ends in `(push)` or `(poll)` depending on whether
the buds announced the change or it was found by polling — which matters, because
**the buds announce a bud leaving your ear but not going in**. A listener that only
consumed pushes would never show `in-ear` unless it started with them worn. So
placement is polled every 3 s (`-i`) and battery/ANC every 60 s
(`--battery-interval`).

The raw placement byte is printed on every line on purpose: those bits are
reverse-engineered, and a mislabelling should carry its own evidence.

### waybar

```jsonc
"custom/buds": {
  "exec": "cmf-buds waybar",
  "return-type": "json",
  "interval": 120,
  "on-click": "bt-reconnect"
}
```

The tooltip carries all three components; `text` shows the lower of the two buds,
which is the number that decides when the music stops. `class` is
`charging` / `critical` (≤ 15 %) / `normal` / `disconnected` for styling.

With `cmf-budsd` running each poll is just a socket read, so the interval is cheap;
without it, every poll opens an RFCOMM link, so keep it coarse. For an event-driven
module, `cmf-buds watch` prints a line per change.

## cmf-budsd

```console
$ systemctl --user status cmf-budsd
17:56:29 listening on /run/user/1000/cmf-buds.sock for 3C:B0:ED:D1:A4:AB
17:56:30 connected on channel 16
```

`cmf-budsd [--socket PATH] [-i SECS] [--battery-interval SECS] [--idle-release SECS] [--allow-raw]`

It holds the one RFCOMM link and serves newline-delimited JSON. On connect a client
is sent the current state immediately, then a full state line on every change —
never deltas, since a line is ~400 bytes at well under 1/s.

```jsonc
{"type":"state","seq":6,"ts":1787241442,"src":"poll","connected":true,"reason":null,
 "battery":{"left":{"percent":100,"charging":false,"live":true,"seen":1787241442},
            "case":{"percent":20,"charging":false,"live":false,"seen":1787241438}},
 "placement":{"left":{"online":true,"where":"in-ear","raw":132}},
 "anc":"transparency","firmware":"1.0.1.52","protocol":"4.0",
 "components":{"left":{"firmware":"1.0.1.52","serial":"1351982513002315"}},
 "host":{"mac":"90:A9:F7:5E:04:B5","name":"T30Pro"}}
```

Clients may send `{"cmd":"refresh"}` (rate-limited to 1/s) and, with `--allow-raw`,
`{"cmd":"raw","command":"c007","id":"…"}`. Replies come back as `{"type":"reply"}`
or `{"type":"error"}`, each echoing the `id` you sent.

### Decisions worth knowing

**It is read-only by construction.** `raw` needs `--allow-raw` *and* refuses any
command outside `0xc000`–`0xcfff`. Any process of your user can open that socket,
and a read sweep once reset the link (see `PROTOCOL.md`), so the write range is
closed off in the one place it can be enforced rather than left to client good
behaviour.

**`seen` is on the wire; an age never is.** Sending an age would make every poll a
"change" and stream forever. Clients subtract from `seen` on their own timer, which
is why the Quickshell panel's "12 m ago" ticks while the socket stays silent — a
stable, connected system emits *zero* lines.

**It never runs `bt-reconnect`.** That tool power-cycles the adapter from its second
attempt, taking every other Bluetooth device down with it, and decides the
multipoint question for you. Those are your calls, so it is wired to the panel
button instead.

**It gates on BlueZ before touching RFCOMM.** A blind connect costs a 6 s timeout,
and repeating that while the case sits shut for hours is the difference between a
daemon that idles and one that thrashes. It reads `org.bluez.Device1.Connected`
first (~20 ms) and only then attempts the link.

**`--idle-release` (60 s by default) drops the link when nothing is listening.**
The buds' single vendor channel is also the one the phone's Nothing X app wants. If
no client is attached there is nothing to know, so the link is given back and
re-taken on the next connection. `--idle-release 0` disables it.

**`EBUSY` gets its own fast backoff** (2, 3, 4, 5 s) separate from the connect
backoff (1, 2, 5, 10, 20, 30 s). Restarting the service is the common case, and the
buds are still holding the previous link when systemd comes back 5 s later.

### Running it

Already installed as a user service:

```bash
systemctl --user enable --now cmf-budsd
journalctl --user -u cmf-budsd -f
```

`~/.config/systemd/user/cmf-budsd.service` is a plain file following the
`swaybg`/`swayidle` convention in that directory — deliberately *not*
`After=bluetooth.target`, which is a system unit and gives a user unit no ordering
anyway. Since `home.nix` is becoming the source of truth here, the equivalent is:

```nix
systemd.user.services.cmf-budsd = {
  Unit = {
    Description = "CMF Buds 2 per-component battery daemon";
    PartOf = [ "graphical-session.target" ];
    After = [ "graphical-session.target" ];
  };
  Service = {
    ExecStart = "%h/.local/bin/cmf-budsd";
    Restart = "always";
    RestartSec = "5";
  };
  Install.WantedBy = [ "graphical-session.target" ];
};
```

## Quickshell

Three files in the dotfiles repo (`~/dotfiles/dot-config/quickshell`), plus two
hooks in `Bar/Bar.qml`. They are not in this repo — the only thing crossing the
boundary is the JSON socket contract:

| File | What |
|---|---|
| `Services/CmfBuds.qml` | singleton: a `Socket` to the daemon, one `Part` object each for left / right / case |
| `Bar/BudsWidget.qml` | braille battery level, coloured, in the indicators pill |
| `Bar/BudsPanel.qml` | the hover-opened panel: three rows, ANC, Reconnect |

The widget sits next to `Volume` and `Stats`; hovering it opens the panel, with the
same 250 ms grace timer the other popups use.

Details that are not obvious:

- **A braille ramp, not a 🎧.** `color` has no effect on the headphones emoji — it
  renders from the colour emoji font — so the level itself carries the colour,
  green → yellow → red, mirroring `Bar/StatBar.qml`'s battery thresholds and ramp.
  There is no percentage text and no placeholder glyph: when the buds are away the
  widget draws nothing at all.
- **Hover, not click**, matching every other icon in this bar. Buttons inside a
  hover-driven popup are fine — the grace timer checks the panel's own hover state,
  which is exactly how `NotificationCenter`'s buttons stay clickable.
- **The reconnect `Timer` cycles `connected` through false.** Assigning `true` alone
  is a no-op once the socket has been disconnected, and the shell then never
  reconnects to a restarted daemon. This was verified the hard way.
- **The widget stays visible while the buds are away** (greyed, showing `--`), and
  hides only when the daemon itself is down. Hiding it when the buds disconnect
  would take the Reconnect button away at exactly the moment you want it.
- **`chargeCase`, not `case`** — `case` is a reserved word in QML.

## bt-reconnect

```console
$ bt-reconnect
==> Stopped a discovery session we owned
==> Connect attempt 1/4
Connected: CMF Buds 2  [BlueZ battery: 95%]
Default sink -> bluez_output.3C_B0_ED_D1_A4_AB.1
```

`bt-reconnect [MAC] [-n TRIES] [-q] [--no-audio] [--reset]`

It drives BlueZ over D-Bus with `busctl` rather than `bluetoothctl`. That matters
for two reasons: the error names come back precisely (`br-connection-page-timeout`
vs `br-connection-busy` vs `NotReady`) instead of as prose, and nothing can ever
block on `bluetoothctl`'s interactive `Scan and connect (yes,no):` prompt.

What it does, in order:

1. Powers the adapter on if it is off, and marks the device trusted so BlueZ will
   reconnect it unprompted in future.
2. Exits immediately if the device is already connected *and* has an A2DP
   transport. `Connected: yes` on its own is not enough — a link with no
   transport is half-open and carries no audio.
3. Stops a leftover discovery session, and warns if one belongs to another client.
4. Retries with escalation: a short scan burst to wake the buds on the first
   failure, an adapter power-cycle from the second.
5. Waits for the A2DP transport, then points the PipeWire default sink at it
   (`--no-audio` skips that).
6. `notify-send` on success and on final failure.

### Why the buds are flaky here, concretely

Both causes are visible in this machine's logs, and the script addresses each:

**A leftover scan session.** The adapter was sitting at `Discovering: yes` with
no one connecting, from a bluetui/`bluetoothctl` session that had exited without
stopping its scan — the log even shows `Failed to start discovery:
org.bluez.Error.InProgress`. Scanning while a connect is in flight measurably
hurts, because the radio is time-slicing between the two. The script stops a
session it owns and tells you when one belongs to somebody else, which is the
case it cannot fix on your behalf — close bluetui.

**Multipoint.** `cmf-buds info` reports which host the buds are talking to, and
over one session it named `Gar CMF`, `Pixel 9a` and this machine at different
moments — they genuinely juggle several. When they are attached elsewhere they
will not answer
this machine's page, which surfaces as `br-connection-page-timeout`, and BlueZ
logs it as `Unable to get Hands-Free Voice gateway SDP record: Host is down`.
No amount of retrying from this side fixes that; the script says so rather than
spinning.

**One genuine bug this exposed:** `Device1.Connect` can take longer than the
25-second D-Bus reply timeout and then *succeed anyway*, after having already
returned an error. A naive script reports failure while the buds are connecting
fine — which is exactly what happened on the first test run here. Every failure
path re-checks `Connected` before giving up.

## sdp-services

```console
$ sdp-services
3C:B0:ED:D1:A4:AB - 13 service records

0x00010003  (unnamed)                    RFCOMM channel 3   0x111e (Handsfree), 0x1203 (Generic Audio)
0x00010007  RFCOMM COM                   RFCOMM channel 17  df21fe2c-2515-4fdb-8886-f12c4d67927c
0x00010008  TOTA                         RFCOMM channel 12  0x1101 (Serial Port)
0x00010009  NTAPP                        RFCOMM channel 16  aeac4a03-dff5-498f-843a-34487cf133eb
0x0001000a  WATCH                        RFCOMM channel 29  99999999-9999-9999-9999-999999999999
0x0001000b  BESOTA                       RFCOMM channel 13  66666666-6666-6666-6666-666666666666
```

`sdp-services [MAC] [UUID ...]`

Fedora moved `sdptool` into `bluez-deprecated` and BlueZ exposes no SDP browse
over D-Bus, so this speaks SDP directly over L2CAP. No root needed.

The catch it handles: **none of the vendor services are in the public browse
group**, so a plain browse finds 7 of the 13 records and misses every interesting
one. With no arguments it searches the browse group, Serial Port, *and* every UUID
BlueZ already cached for the device, which recovers the full set.

## Installing

The scripts are self-contained; symlink them onto `PATH`:

Through nix, which also fixes the interpreter:

```bash
nix build ../..#budskit      # or: just build
```

For home-manager, add the flake as an input and then:

```nix
home.packages = [ tools.packages.${pkgs.system}.budskit ];

systemd.user.services.cmf-budsd = {
  Unit = {
    Description = "CMF Buds 2 per-component battery daemon";
    PartOf = [ "graphical-session.target" ];
    After = [ "graphical-session.target" ];
  };
  Service = {
    ExecStart = "${tools.packages.${pkgs.system}.budskit}/bin/cmf-budsd";
    Restart = "always";
    RestartSec = "5";
  };
  Install.WantedBy = [ "graphical-session.target" ];
};
```

`systemd/cmf-budsd.service` is the same unit as a plain file, for a non-nix install.

For a dev checkout, `just install` symlinks `bin/` into `~/.local/bin`. `cmfbuds.py`
is imported from beside the scripts, which resolve their own symlink first, so the
links work without copying it.

The waybar `text` uses a Nerd Font charging glyph (`󰥉`); swap it for `⚡` in
`cmd_waybar` if your bar font has no Nerd Font coverage.

## Possible next steps

- Register a BlueZ **Battery Provider** (`org.bluez.BatteryProvider1`) from
  `cmf-budsd`, so the three levels appear in UPower and every desktop battery
  applet gets them for free. The daemon already has the state; this is just an
  extra publisher on the same data.
- Decode the rest of the `0xc0…` replies listed as unknown in `PROTOCOL.md` —
  in-ear detection, low-latency mode and the EQ are all in there.
- Add write support (ANC switching, find-my-buds) using the `0xf…` commands.
  Gadgetbridge already documents `0xf002` / `0xf004` / `0xf00f`.
