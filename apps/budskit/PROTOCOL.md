# CMF Buds 2 — vendor protocol notes

Everything here was observed against a real device:

```
CMF Buds 2   3C:B0:ED:D1:A4:AB
firmware     1.0.1.52       protocol string "4.0"
chipset      BES (judging by the TOTA/BESOTA service names)
host         Fedora 43, BlueZ 5.87
```

The framing comes from Gadgetbridge's `NothingProtocol.java` (AGPL-3.0), which
implements Nothing Ear (1)/(2)/(stick) and CMF Buds Pro 2. CMF Buds 2 is not in
Gadgetbridge's device list, but it speaks the same protocol on the same UUID.
Everything below the "Commands" heading was read off this device directly.

## Reaching the device

`bluetoothctl info` advertises four vendor UUIDs, and the buds publish six RFCOMM
services. None of them are in the public browse group, so a plain SDP browse
misses them — you have to search for the UUID (or for `0x1101`, Serial Port,
which brings back four of them at once).

| SDP record name | Service class | RFCOMM channel |
|---|---|---|
| `NTAPP`      | `aeac4a03-dff5-498f-843a-34487cf133eb` | **16** |
| `RFCOMM COM` | `df21fe2c-2515-4fdb-8886-f12c4d67927c` | 17 |
| `BESOTA`     | `66666666-6666-6666-6666-666666666666` | 13 |
| `TOTA`       | `0x1101` (Serial Port)                 | 12 |
| `WATCH`      | `99999999-9999-9999-9999-999999999999` | 29 |
| (HFP)        | `0x111e`, `0x1203`                     | 3 |

Plus A2DP sink on L2CAP PSM 25, AVRCP on PSM 23 and GATT on PSM 31. Use
`bin/sdp-services` to reproduce the whole table.

`NTAPP` on channel 16 is the one the Nothing X app uses and the only one that
answers this protocol. `TOTA` on channel 12 accepts a connection and then stays
silent; the OTA channels are firmware-update transports — leave them alone.

**Do not hardcode channel 16.** It is assigned by the firmware and other Nothing
models use other numbers (channel 15 is what the Ear (2) write-ups report).
`bin/cmf-buds` resolves it with its own SDP `ServiceSearchAttribute` query over
raw L2CAP, which needs no `sdptool` and no root.

Two constraints worth knowing:

- The buds accept **one** `NTAPP` connection at a time, and the previous one takes
  a few seconds to tear down — an immediate second run gets `EBUSY` (errno 16).
- The buds must be connected *and* not shut inside the case. A closed case drops
  the ACL link entirely and nothing is reachable.

## Frame format

```
  0x55  <control:u16le>  <command:u16le>  <length:u16le>  <fsn:u8>  <payload>  [crc:u16le]
```

- **control** — `0x5120` works for every query here. Bit layout, from Gadgetbridge:
  `& 0x0F00 >> 8` is the device type (`1` = TWS headset), `& 0x1F` is the response
  code (non-zero ⇒ the device rejected the request), `& 0x20` means a CRC is
  appended. Some models also want `| 0x40` and an incrementing `fsn`; CMF does not
  (`CmfBudsPro2Coordinator.incrementCounter()` returns false, and `fsn = 0` works
  here).
- **command** — requests carry the high bit set (`0xc007`); the reply comes back
  with it cleared (`0x4007`). OR `0x8000` back in when matching them up.
- **length** — payload length, excluding the CRC.
- **crc** — CRC-16/ARC (poly `0xA001` reflected, seed `0xFFFF` — i.e. Modbus)
  over the whole frame *before* the CRC, appended little-endian. Gadgetbridge
  calls this `getCRC16ansi`.

Battery request, byte for byte:

```
55 20 51 07 c0 00 00 00 20 8b
│  └──┬──┘ └──┬──┘ └──┬──┘ │  └──┬──┘
│   0x5120  0xc007   len=0 fsn  CRC
SOF
```

## Commands

All read with `control=0x5120` and an empty payload. Replies below are verbatim.

### Confirmed

| Command | Reply payload | Meaning |
|---|---|---|
| `0xc007` | `02 02 5f 03 5f` | **Battery levels** |
| `0xc00a` | `03 02 81 03 84 04 01` | **Placement** — in case / in ear / neither |
| `0xc006` | `04` + CSV text | **Per-bud firmware and serial** |
| `0xc042` | `31 2e 30 2e 31 2e 35 32` | Firmware version, ASCII `1.0.1.52` |
| `0xc001` | `34 2e 30 00` | Protocol version, ASCII `4.0` |
| `0xc01e` | `01 07 00 02 01 00` | **ANC / audio mode** |
| `0xc028` | `05 01 01 01` + entries | **Multipoint paired-device list** |
| `0xc018` | 33 bytes | Touch-gesture map (see below) |

### Pushed without being asked

| Event | Payload | Meaning |
|---|---|---|
| `0xe001` | same encoding as `0xc007` | battery changed |
| `0xe002` | same encoding as `0xc00a` | a bud moved / lid opened / lid closed |
| `0xe003` | same encoding as `0xc01e` | ANC mode changed |

The buds narrate most of their own state changes, so a status bar can largely be
event-driven — but **not entirely**, because the push set has a hole:

> `0xe002` fires when a bud is taken **out** of an ear, and does not fire when one
> is put **in**. Polling `0xc00a` at that moment reports `in-ear` correctly, so the
> state is tracked internally and simply not announced.

That makes a purely event-driven watcher never show `in-ear` unless it happened to
start with the buds already worn. `cmf-buds watch` therefore polls `0xc00a` on a
timer *as well as* listening, and suppresses output when the decoded state has not
changed.

### Answered, not decoded

| Command | Reply payload | Guess |
|---|---|---|
| `0xc00c` | `01` | some status flag; once coincided with a burst of pushes |
| `0xc00d` | `43 60 92` | — |
| `0xc00e` | `01 01 01` | three booleans (in-ear detection / low-latency / ultra bass?) |
| `0xc01c` | `79 b1` | — |
| `0xc01f` | `00` | — |
| `0xc027` | `01` | — |
| `0xc029` | `00` | — |
| `0xc041` | `02` | — |
| `0xc044` | 44 bytes of IEEE floats | custom EQ (frequency / gain / Q triplets) |
| `0xc04e` | `01 02` | — |
| `0xc04f` | `00` | — |
| `0xc050` | `00` | — |

Commands in `0xc000`–`0xc0af` that are not listed reply with an empty payload.

### A warning about sweeping

Reading unknown IDs is not free. During a sweep of `0xc040`–`0xc0af` the buds
pushed `0xe003` audio-mode changes and then reset the RFCOMM link at `0xc073`.
The `0xf…` range is where Gadgetbridge's *write* commands live (`0xf002` find
device, `0xf004` in-ear detection, `0xf00f` set audio mode), so treat anything
there as an action, not a query.

## Payload encodings

### Battery — `0xc007`, `0xe001`

```
<count:u8>  then count × ( <component:u8> <value:u8> )

component   0x02 left bud   0x03 right bud   0x04 case
value       bits 0-6 = percent, bit 7 = charging
```

`02 02 5f 03 5f` → 2 entries: left 95 %, right 95 %.

**Only components that are currently reporting appear.** With one bud in the case
you get a single entry; with the case docked and reporting you get three. There
is no request that forces all three — the case level arrives on its own in an
`0xe001` push around docking events. `cmf-buds` therefore caches the last value
seen per component with a timestamp, and renders the case as
`(last seen 20m ago)` rather than dropping it.

Observed with everything reporting: `03 02 64 03 5f 04 0f` → left 100 %,
right 95 %, case 15 %.

Bit 7 has not been seen set on this device, but the buds it was checked against
turned out to be discharging at the time, so there is no evidence against it
either. `0xc00a` carries no charging flag — see the note under Placement.

### Placement — `0xc00a`, `0xe002`

Same `count, (component, value)` shape. For the two buds:

| Bits | Meaning |
|---|---|
| `0x80` | online / reachable |
| `0x07` | where the bud is: `0x01` in the case, `0x02` out of case and out of ear, `0x04` in an ear |
| `0x10` | unknown — see below |

Verified by reading decoded output back against the actual physical situation,
one bud at a time:

| Frame | left (`0x02`) | right (`0x03`) | Confirmed situation |
|---|---|---|---|
| `03 02 81 03 81 04 01` | `0x81` | `0x81` | both in the case |
| `03 02 81 03 84 04 01` | `0x81` | `0x84` | left in the case, right in an ear |
| `03 02 84 03 81 04 01` | `0x84` | `0x81` | left in an ear, right in the case |
| `03 02 82 03 82 00 00` | `0x82` | `0x82` | both out of the case, not worn |
| `03 02 84 03 82 00 00` | `0x84` | `0x82` | both in ears — right byte lags, see below |

Those frames also pin the component indices down: `0x02` really is left and
`0x03` right, since the two middle rows are mirror images of each other.

**Two earlier readings of this byte were wrong, in opposite directions.** First
`0x01` was taken for "in-ear" and `0x04` for "in-case"; then `0x10` was taken for
"docked". `0x01` and `0x04` are in fact the exact opposite of the first guess.
Nothing about the bit pattern reveals that — it only came out of checking labels
against buds that were physically somewhere known, which is the only method that
works here.

`0x10` appears on some in-ear reports (`0x94`). It is **not** charging: it was
seen on buds whose level was falling from 95 % to 90 %. Its meaning is unknown, so
the decoder ignores it and `watch` prints the raw byte instead.

The one frame that does not match is `0x82` on a bud that was in an ear. Because
these frames are pushed on transitions, the likely explanation is that the report
is emitted before in-ear detection has asserted — the same bud reads `0x84` once
settled. Not confirmed.

The case's slot is `00 00` when it is not reporting and `0x01` when it is. That
does not follow the bud encoding at all (reading it as "in-ear" was a third bug).
Notice which frames above carry `04 01`: **the case only reports while at least
one bud is docked in it**, which is also why its battery level is only
intermittently available.

Charging comes from bit 7 of the battery byte in `0xc007` and nowhere else. It has
not yet been observed set on this device.

### Per-bud firmware and serial — `0xc006`

A count byte followed by ASCII lines of `<component>,<field>,<value>`, where
`component` reuses the battery indices and `field` is `2` for firmware and `4`
for serial:

```
04
2,2,1.0.1.52
2,4,1351982513002315
3,2,1.0.1.52
3,4,1351982513002315
```

Both buds report the same serial on this device.

### ANC / audio mode — `0xc01e`, `0xe003`

Payload `01 <mode> 00 02 01 00`; the trailing bytes are CMF-specific and were
constant here. Mode values, from Gadgetbridge:

| Value | Mode |
|---|---|
| `0x01` | ANC high |
| `0x03` | ANC low |
| `0x05` | off |
| `0x07` | transparency |

### Connected host — `0xc028`

A 4-byte header then `<6-byte MAC, byte-reversed> <name-length> <name>`:

```
05 05 01 11  9f 56 40 9e ff 60  03  67 61 72
             └── 60:FF:9E:40:56:9F ─┘  3   "gar"
```

It returns exactly **one** record, and it is constant for the life of a
connection — five queries down one link gave the same answer every time. So this
is the host on the other end of the link, not a list of slots (an earlier version
of these notes called it a "multipoint list", which was wrong).

Across this session it nonetheless came back as three different hosts:

| Header | Host |
|---|---|
| `05 05 01 11` | `60:FF:9E:40:56:9F` `gar` — this machine |
| `05 01 01 01` | `3C:B0:ED:2A:0F:EE` `Gar CMF` |
| `05 03 01 00` | `B0:D5:FB:A3:22:B6` `Pixel 9a` |
| — | `90:A9:F7:5E:04:B5` `T30Pro` |

Which is still the point for reconnect trouble: the buds do juggle several hosts,
so they are not always available to this one. The second header byte varies with
the host (`01`/`03`/`05`) and looks like a slot index; the fourth (`01`/`00`/`11`)
looks like flags, `0x11` being the host actually connected. Unconfirmed.

### Touch gestures — `0xc018`

A count byte (`08`) then 8 × 4 bytes of `<side> 01 <gesture> <action>`, with
`side` reusing `0x02` left / `0x03` right:

```
08
03 01 02 09   03 01 03 08   03 01 07 16   03 01 09 01
02 01 02 09   02 01 03 08   02 01 07 16   02 01 09 01
```

Four gestures per side (`0x02`, `0x03`, `0x07`, `0x09`) mapped to four actions
(`0x09`, `0x08`, `0x16`, `0x01`). Which is which was not tested.

## What BlueZ itself gives you, for comparison

BlueZ exposes `org.bluez.Battery1.Percentage`, a single aggregated number
(`0x5f` = 95 in the runs above), which is why the case and the individual buds
are invisible through it. It picks that up from the HFP `AT+IPHONEACCEV`
indicator and from the Google Fast Pair advertisement.

That advertisement is worth knowing about because it needs **no connection at
all** — it is in `ServiceData` under `0000fe2c-…`:

```
00 40 28 d4 00 20 11 cc 34 7f 64 7f
│  └───── account key filter ─────┘ │  └── batteries ──┘
│  0x40 = len 4, type 0             │  0x34 = len 3, type 4 (hide UI)
version/flags            0x11 = len 1, type 1 (salt)
```

The three battery bytes are left, right, case with the same
`bit7 = charging, bits 0-6 = percent` encoding and `0x7f` meaning unknown. Above:
left unknown, right 100 %, case unknown. A passive BLE scanner could read the
buds' battery without ever connecting — but in practice the values are `0x7f`
most of the time, so the RFCOMM route is the reliable one.

## Sources

- Gadgetbridge `NothingProtocol.java`, `AbstractEarCoordinator.java`,
  `CmfBudsPro2Coordinator.java`, `CheckSums.java` — framing, CRC, battery
  encoding, component indices, ANC values.
  <https://codeberg.org/Freeyourgadget/Gadgetbridge>
- Bharadwaj Raju, "Creating a Linux controller for the Nothing Ear (2)" — the
  `55 60 01` framing and the RFCOMM-channel-15 finding for that model.
  <https://bharadwaj-raju.github.io/posts/nothing-ear-2-on-linux/>
- Google Fast Pair non-discoverable advertisement format (battery section
  types `0b0011` / `0b0100`).
