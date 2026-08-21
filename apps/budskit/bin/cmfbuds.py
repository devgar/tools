"""Protocol and transport for CMF / Nothing earbuds over their vendor SPP channel.

Shared by `cmf-buds` (the CLI) and `cmf-budsd` (the daemon), which must not carry
two copies of the framing. See PROTOCOL.md for how any of this was established.

Errors are raised, never exited on: the daemon has to survive a case being closed,
which happens many times a day, so turning a dropped link into `SystemExit` belongs
in the CLI and nowhere else.
"""

from __future__ import annotations

import json
import os
import select
import socket
import struct
import time
from pathlib import Path

DEFAULT_MAC = os.environ.get("CMF_BUDS_MAC", "3C:B0:ED:D1:A4:AB")

NTAPP_UUID = "aeac4a03-dff5-498f-843a-34487cf133eb"
NTAPP_FALLBACK_CHANNEL = 16

CACHE_DIR = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "cmf-buds"

# ---------------------------------------------------------------- errors


class BudsError(Exception):
    """Anything that went wrong talking to the buds."""


class BudsBusy(BudsError):
    """EBUSY - the buds accept one NTAPP link and have not released the last one."""


class BudsGone(BudsError):
    """The link dropped mid-stream, which is what a closed case looks like."""


# ---------------------------------------------------------------- protocol

SOF = 0x55
CONTROL_QUERY = 0x5120          # device=TWS headset, CRC present
CONTROL_CRC_FLAG = 0x20
MASK_RSP_CODE = 0x1F

CMD_PROTOCOL_VERSION = 0xC001
CMD_COMPONENT_INFO = 0xC006     # per-bud firmware + serial, as CSV text
CMD_BATTERY = 0xC007
CMD_WEAR_STATE = 0xC00A
CMD_FIRMWARE = 0xC042
CMD_ANC_MODE = 0xC01E
CMD_PAIRED_DEVICES = 0xC028

EVT_BATTERY = 0xE001            # pushed unsolicited whenever a level changes
EVT_WEAR_STATE = 0xE002
EVT_ANC_MODE = 0xE003

# Replies to a query keep the 0xc000 form; unsolicited pushes use 0xe000. That one
# bit is the only thing distinguishing "we asked" from "the buds volunteered".
PUSH_BIT = 0x2000

# Everything this project sends is a query. Writes live in 0xf000 and are
# deliberately unimplemented - see PROTOCOL.md.
QUERY_RANGE = range(0xC000, 0xD000)

COMPONENTS = {0x02: "left", 0x03: "right", 0x04: "case"}
COMPONENT_ORDER = ("left", "right", "case")

ANC_MODES = {0x01: "anc-high", 0x03: "anc-low", 0x05: "off", 0x07: "transparency"}

# Placement byte (0xc00a / 0xe002): 0x80 = online, low 3 bits = where the bud is.
# Established by checking decoded output against the physical situation, which is
# the only way to get this right - two earlier guesses had 0x01 and 0x04 swapped.
PLACEMENT_ONLINE = 0x80
PLACEMENT_WHERE = {0x01: "in-case", 0x02: "out-of-ear", 0x04: "in-ear"}

# 0x10 rides along on some in-ear reports (0x94). It is NOT charging: it was seen
# on buds whose level was falling. Meaning still unknown, so it is only surfaced
# through the raw byte.
PLACEMENT_UNKNOWN = 0x10

# A desynced stream must not grow a buffer forever in a process that lives for weeks.
MAX_TAIL = 8192


def crc16_modbus(data: bytes) -> int:
    """CRC-16/ARC with 0xFFFF seed - Gadgetbridge calls this getCRC16ansi()."""
    crc = 0xFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc & 0xFFFF


def encode(command: int, payload: bytes = b"", control: int = CONTROL_QUERY, fsn: int = 0) -> bytes:
    msg = bytes([SOF]) + struct.pack("<HHHB", control, command, len(payload), fsn) + payload
    if control & CONTROL_CRC_FLAG:
        msg += struct.pack("<H", crc16_modbus(msg))
    return msg


class Frame:
    __slots__ = ("control", "command", "fsn", "payload", "crc_ok")

    def __init__(self, control, command, fsn, payload, crc_ok):
        self.control, self.command, self.fsn = control, command, fsn
        self.payload, self.crc_ok = payload, crc_ok

    @property
    def ok(self) -> bool:
        return (self.control & MASK_RSP_CODE) == 0

    @property
    def usable(self) -> bool:
        """Both checks. A long-lived process cannot afford to trust a bad CRC."""
        return self.ok and self.crc_ok

    @property
    def pushed(self) -> bool:
        return bool(self.command & PUSH_BIT)

    def __repr__(self) -> str:
        return f"<0x{self.command:04x} {self.payload.hex(' ') or '-'}>"


def decode(buf: bytes) -> tuple[list[Frame], bytes]:
    """Split a byte stream into frames; return (frames, unconsumed tail)."""
    frames, i = [], 0
    while True:
        start = buf.find(bytes([SOF]), i)
        if start < 0 or len(buf) - start < 8:
            break
        control, command, length = struct.unpack("<HHH", buf[start + 1:start + 7])
        crc_len = 2 if control & CONTROL_CRC_FLAG else 0
        end = start + 8 + length + crc_len
        if end > len(buf):
            break
        body = buf[start:start + 8 + length]
        crc_ok = True
        if crc_len:
            crc_ok = struct.unpack("<H", buf[end - 2:end])[0] == crc16_modbus(body)
        # Replies clear the high bit that requests carry; normalise back to 0x8000.
        frames.append(Frame(control, command | 0x8000, buf[start + 7], body[8:], crc_ok))
        i = end
    return frames, buf[i:]


# ---------------------------------------------------------------- transport


def find_channel(mac: str) -> int:
    """Resolve the NTAPP RFCOMM channel via SDP; the number is not stable across models."""
    cache = CACHE_DIR / f"{mac.replace(':', '')}.channel"
    try:
        return int(cache.read_text().strip())
    except (OSError, ValueError):
        pass
    try:
        channel = sdp_rfcomm_channel(mac, NTAPP_UUID)
    except OSError:
        channel = None
    channel = channel or NTAPP_FALLBACK_CHANNEL
    try:
        CACHE_DIR.mkdir(parents=True, exist_ok=True)
        cache.write_text(str(channel))
    except OSError:
        pass
    return channel


def sdp_rfcomm_channel(mac: str, uuid: str, timeout: float = 8.0) -> int | None:
    """Minimal SDP ServiceSearchAttribute query over raw L2CAP - no sdptool needed."""
    raw_uuid = bytes.fromhex(uuid.replace("-", ""))
    pattern = bytes([0x18 | 4]) + raw_uuid
    params = (
        b"\x35" + bytes([len(pattern)]) + pattern
        + struct.pack(">H", 0x0280)
        + b"\x35\x05\x0a\x00\x04\x00\x04"      # attribute range 0x0004 (ProtocolDescriptorList)
        + b"\x00"
    )
    sock = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_SEQPACKET, socket.BTPROTO_L2CAP)
    sock.settimeout(timeout)
    try:
        sock.connect((mac, 1))
        sock.send(struct.pack(">BHH", 0x06, 1, len(params)) + params)
        resp = sock.recv(4096)
    finally:
        sock.close()
    if len(resp) < 8 or resp[0] != 0x07:
        return None
    # The RFCOMM channel is the single-byte uint that follows the RFCOMM UUID (0x0003).
    blob = resp[7:]
    marker = b"\x19\x00\x03"                    # uuid16 0x0003
    pos = blob.find(marker)
    if pos < 0 or pos + 4 >= len(blob):
        return None
    return blob[pos + 4]


class Buds:
    def __init__(self, mac: str = DEFAULT_MAC, channel: int | None = None, timeout: float = 6.0):
        self.mac = mac.upper()
        self._channel = channel
        self.timeout = timeout
        self.sock: socket.socket | None = None
        self.desyncs = 0
        self._tail = b""

    @property
    def channel(self) -> int:
        """Resolved lazily: SDP costs seconds, and the daemon must not pay it while
        the buds are away."""
        if self._channel is None:
            self._channel = find_channel(self.mac)
        return self._channel

    def connect(self, retries: int = 4) -> "Buds":
        last: OSError | None = None
        for attempt in range(max(1, retries)):
            sock = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM)
            sock.settimeout(self.timeout)
            try:
                sock.connect((self.mac, self.channel))
                self.sock = sock
                self._tail = b""
                return self
            except OSError as exc:
                sock.close()
                last = exc
                if exc.errno != 16:  # EBUSY
                    break
                if attempt < retries - 1:
                    time.sleep(2 + attempt)
        detail = f"{self.mac} ch {self.channel}: {last}"
        if last is not None and last.errno == 16:
            raise BudsBusy(f"the NTAPP channel is already in use ({detail})")
        raise BudsError(
            f"cannot open the NTAPP channel ({detail})\n"
            "The buds must be connected and not shut in the case. Try: bt-reconnect"
        )

    def __enter__(self) -> "Buds":
        return self.connect()

    def __exit__(self, *_):
        self.close()

    def close(self) -> None:
        if self.sock:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None

    def set_nonblocking(self) -> None:
        """For the daemon: the socket stays non-blocking for its whole life, so no
        code path can stall the event loop."""
        assert self.sock
        self.sock.setblocking(False)

    def send(self, command: int, payload: bytes = b"", control: int = CONTROL_QUERY) -> None:
        if not self.sock:
            raise BudsGone("not connected")
        try:
            self.sock.sendall(encode(command, payload, control))
        except OSError as exc:
            raise BudsGone(f"send failed: {exc}") from exc

    def _feed(self, chunk: bytes) -> None:
        self._tail += chunk
        if len(self._tail) > MAX_TAIL:
            # Resync on the most recent start-of-frame; if there is not one to fall
            # back to, the stream is unusable and keeping it would leak.
            cut = self._tail.rfind(bytes([SOF]))
            self._tail = self._tail[cut:] if cut > 0 else b""
            self.desyncs += 1

    def read(self, window: float = 1.2) -> list[Frame]:
        """Collect every frame that arrives within `window` seconds. Blocking - CLI only."""
        if not self.sock:
            raise BudsGone("not connected")
        frames: list[Frame] = []
        deadline = time.monotonic() + window
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            self.sock.settimeout(remaining)
            try:
                chunk = self.sock.recv(2048)
            except (socket.timeout, TimeoutError):
                break
            except OSError:
                break
            if not chunk:
                break
            self._feed(chunk)
            new, self._tail = decode(self._tail)
            frames.extend(new)
        return frames

    def query(self, command: int, payload: bytes = b"", window: float = 1.2) -> list[Frame]:
        self.send(command, payload)
        return self.read(window)

    def drain_nb(self) -> list[Frame]:
        """Decode whatever is buffered, without waiting or touching the socket mode.

        Raises BudsGone when the peer went away - the caller decides what that means.
        """
        if not self.sock:
            raise BudsGone("not connected")
        while True:
            try:
                chunk = self.sock.recv(4096)
            except (BlockingIOError, InterruptedError):
                break
            except OSError as exc:
                raise BudsGone(f"link dropped: {exc}") from exc
            if not chunk:
                raise BudsGone("the buds closed the channel")
            self._feed(chunk)
        frames, self._tail = decode(self._tail)
        return frames

    def wait_readable(self, timeout: float) -> bool:
        if not self.sock:
            raise BudsGone("not connected")
        return bool(select.select([self.sock], [], [], timeout)[0])


# ---------------------------------------------------------------- decoders


def decode_battery(payload: bytes) -> dict[str, dict]:
    """payload: count, then (component, value) pairs. Bit 7 of value = charging."""
    out: dict[str, dict] = {}
    count = payload[0] if payload else 0
    for i in range(count):
        base = 1 + 2 * i
        if base + 1 >= len(payload):
            break
        idx, value = payload[base], payload[base + 1]
        name = COMPONENTS.get(idx)
        if name is None or idx == 0:
            continue
        out[name] = {"percent": value & 0x7F, "charging": bool(value & 0x80)}
    return out


def decode_wear(payload: bytes) -> dict[str, dict]:
    out: dict[str, dict] = {}
    count = payload[0] if payload else 0
    for i in range(count):
        base = 1 + 2 * i
        if base + 1 >= len(payload):
            break
        idx, value = payload[base], payload[base + 1]
        name = COMPONENTS.get(idx)
        if name is None or idx == 0:
            continue
        online = bool(value & PLACEMENT_ONLINE)
        if name == "case":
            # The case slot does not use the bud encoding at all: it is 0x01 whenever
            # the case is reporting and absent otherwise, and it only reports while at
            # least one bud is docked in it.
            out[name] = {"online": online,
                         "where": "present" if value else "absent", "raw": value}
            continue
        out[name] = {
            "online": online,
            "where": PLACEMENT_WHERE.get(value & 0x07, f"0x{value & 0x07:02x}"),
            "raw": value,
        }
    return out


def fmt_placement(info: dict, show_raw: bool = False) -> str:
    # The case has no online bit of its own - saying "offline" about a case that is
    # actively reporting its battery was nonsense.
    if info["where"] in ("present", "absent"):
        text = "reporting" if info["where"] == "present" else "absent"
    else:
        text = ("online" if info["online"] else "offline") + ", " + info["where"]
    return f"{text} =0x{info['raw']:02x}" if show_raw else text


def decode_component_info(payload: bytes) -> dict[str, dict]:
    """CSV-ish text: "<component>,<field>,<value>" lines. field 2 = firmware, 4 = serial."""
    fields = {2: "firmware", 4: "serial"}
    out: dict[str, dict] = {}
    for line in payload[1:].decode("ascii", "replace").splitlines():
        parts = line.strip().split(",")
        if len(parts) != 3:
            continue
        try:
            comp, field = int(parts[0]), int(parts[1])
        except ValueError:
            continue
        name = COMPONENTS.get(comp)
        if name and field in fields:
            out.setdefault(name, {})[fields[field]] = parts[2]
    return out


def decode_paired(payload: bytes) -> list[dict]:
    """Host records of <6-byte MAC (reversed)><len><name>.

    0xc028 returns exactly one - the host on the other end of this link. It stays
    constant for the life of a connection, but has come back as the phone and as a
    second phone at other times, so the buds do track several.
    """
    devices, i = [], 0
    while i + 8 <= len(payload):
        mac = payload[i:i + 6][::-1]
        if not any(mac):
            break
        name_len = payload[i + 6]
        name = payload[i + 7:i + 7 + name_len].decode("utf-8", "replace")
        devices.append({"mac": ":".join(f"{b:02X}" for b in mac), "name": name})
        i += 7 + name_len
    return devices


def decode_anc(payload: bytes) -> str:
    if len(payload) >= 2:
        return ANC_MODES.get(payload[1], f"unknown(0x{payload[1]:02x})")
    return "unknown"


# ---------------------------------------------------------------- state cache

# The case only reports while a bud is docked in it, so its level would otherwise
# read as unknown most of the time. Remember the last value seen, with a timestamp.


def cache_path(mac: str) -> Path:
    return CACHE_DIR / f"{mac.replace(':', '')}.json"


def load_cache(mac: str) -> dict:
    try:
        return json.loads(cache_path(mac).read_text())
    except (OSError, ValueError):
        return {}


def save_cache(mac: str, batteries: dict, now: int | None = None) -> None:
    """Merge levels into the cache. Written atomically: this used to be a plain
    write_text, so a concurrent reader could catch a truncated file."""
    if not batteries:
        return
    data = load_cache(mac)
    stamp = int(time.time()) if now is None else now
    for name, info in batteries.items():
        data[name] = {"percent": info["percent"], "charging": info.get("charging", False),
                      "seen": stamp}
    path = cache_path(mac)
    try:
        CACHE_DIR.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(data))
        os.replace(tmp, path)
    except OSError:
        pass


def fmt_age(seconds: float) -> str:
    seconds = int(max(0, seconds))
    if seconds < 90:
        return f"{seconds}s ago"
    if seconds < 5400:
        return f"{seconds // 60}m ago"
    if seconds < 172800:
        return f"{seconds // 3600}h ago"
    return f"{seconds // 86400}d ago"


# ---------------------------------------------------------------- BlueZ


def bluez_connected(mac: str, adapter: str = "hci0") -> bool | None:
    """Is BlueZ holding an ACL to the device? None if that cannot be determined.

    Worth the 20ms: a blind RFCOMM connect costs a 6s timeout, and repeating that
    while the case sits shut for hours is the difference between a daemon that idles
    and one that thrashes. For SPP the ACL is all that matters, so unlike
    bt-reconnect this deliberately does not also require an A2DP transport.
    """
    import subprocess

    path = f"/org/bluez/{adapter}/dev_" + mac.upper().replace(":", "_")
    try:
        proc = subprocess.run(
            ["busctl", "--no-pager", "get-property", "org.bluez", path,
             "org.bluez.Device1", "Connected"],
            capture_output=True, text=True, timeout=3,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout.strip().endswith("true")
