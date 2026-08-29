# UDP IPC Message Contract

Normative reference for the relay's control/observability surface. This is the
primary interface used for end-to-end testing; see `AGENTS.md` → "Interfaces"
for a short pointer back here.

Source of truth: `process_udp_messages` in `src/main.rs`.

## Transport

- **Protocol**: UDP (User Datagram Protocol).
- **Default bind address**: `127.0.0.1:9898`.
- **Configurable via**: `--udp-addr` flag or `THALAMIC_UDP_ADDR` env var (CLI
  flag takes precedence). Accepts any valid `ip:port` socket address.
  **The socket is unauthenticated**: `Stimuli` and `LearningReward` are
  accepted, and `GetNeuroState` is answered, for any sender that can reach
  the bound address, with no credential check. **Traffic is also cleartext**:
  plain UDP carries the JSON with no confidentiality or integrity
  protection, so anything on-path between client and relay can read or
  modify messages in transit. The `127.0.0.1` default keeps this
  loopback-only. Binding to a non-loopback address exposes control of the
  relay to anything on that network; a firewall alone only restricts *who*
  can reach the socket; it does not add confidentiality or integrity for
  the traffic itself. Non-loopback deployments should tunnel this traffic
  through an authenticated, encrypted channel (e.g. a WireGuard/SSH tunnel
  or VPN) rather than relying on network access control alone.
- **Message framing**: one JSON object per UDP datagram, parsed with
  `serde_json::from_str` against the full datagram body. A datagram
  containing two concatenated JSON objects (e.g. `{...}{...}`) fails to
  parse (`serde_json` rejects the trailing characters after the first
  value), so **do not batch multiple JSON objects in a single datagram**.
  Trailing whitespace after the JSON value — including a trailing `\n` — is
  accepted by the parser; the recommendation to send newline-free datagrams
  is about avoiding accidental multi-value framing, not a hard parser
  requirement.
- **Encoding**: UTF-8 text. Non-UTF-8 datagrams are silently dropped.
- **Max datagram size read**: 4096 bytes per receive; a larger datagram is
  truncated to that buffer size by the OS/socket read, and the discarded
  tail is invisible to the relay. This usually makes the truncated bytes
  fail to parse as JSON, but **not always**: if the first 4096 bytes happen
  to contain one complete, well-formed JSON value followed only by
  whitespace, the parser accepts it as a normal message even though the
  datagram was oversized and had additional (silently dropped) content
  after it. Keep every datagram comfortably under 4096 bytes; do not rely
  on oversized datagrams being rejected.
- The relay drains all pending datagrams each step-loop tick (non-blocking
  socket) and processes them in the order the local socket returns them
  that tick. **UDP itself gives no cross-datagram guarantees**: datagrams
  can be dropped, duplicated, or delivered out of order, and this API has
  no acknowledgement, sequencing, or retry mechanism. A dropped reset
  leaves a `Stimuli` pulse active indefinitely (see the persistence note
  below); a reordered reset can erase a pulse before it lands. This risk is
  highest on a non-loopback network path, but **loopback does not
  eliminate it**: the relay only drains the socket once per step-loop tick,
  so a burst of datagrams sent faster than that can still overflow the
  kernel's finite UDP receive buffer and drop packets even on `127.0.0.1`.
  Applications that need reliable, ordered delivery must build that on top
  of this protocol themselves (application-level acknowledgement, sequence
  numbers, and retries — this API provides none of them). Pacing sends to
  roughly the relay's step interval is a separate, best-effort mitigation
  for the receive-buffer-overflow case above; it does nothing for
  ordinary network-level packet loss, duplication, or reordering, so it is
  not a substitute for acknowledgement/sequencing when real reliability is
  required. Loopback only reduces overall risk, it does not remove it.

## Error behavior

The relay is intentionally lenient and never panics on malformed input:

- A datagram that is not valid UTF-8 is dropped.
- A datagram that is not valid JSON is dropped.
- A JSON object with a `"type"` that isn't a recognized value (or has no
  `"type"` field at all) is dropped.
- A recognized message with missing or malformed fields does **not** cause
  the whole message to be dropped in general — behavior is per-field and
  per-message-type (e.g. `Stimuli` clamps out-of-range numbers and coerces
  non-number elements to `0.0`; `LearningReward` defaults missing/malformed
  deltas to `0.0`). See the per-message sections below for the exact rule in
  each case; `Stimuli` is the one type where a structurally invalid `values`
  field (missing, or not an array) does drop the whole message.
- **No error replies are sent.** The only message type that sends any
  response is `GetNeuroState`. All other messages are fire-and-forget: there
  is no ack, and no error is returned to the sender on failure.

## Message types

All requests are JSON objects with a `"type"` field selecting one of the
following.

### `Stimuli`

Sets the SNN's input channel values. **These values persist across every
subsequent SNN step** — the relay does not clear or decay them after a
step — so a one-shot pulse is not automatic: a channel keeps contributing
whatever value it was last set to on every future step until a later
`Stimuli` message overwrites that channel (e.g. with `0.0`).

To reset a channel after a pulse, the reset must land in a **later** step
tick than the pulse, not just a later message. `process_udp_messages`
drains every pending datagram from the socket in one pass before the
single `network.step` call for that tick; two `Stimuli` messages sent back
to back (pulse then reset) can both be drained in the same tick, so the
reset overwrites the pulse before the network ever observes it. There is no
acknowledgement or step-identifier in this API to synchronize on, so a
client cannot reliably guarantee the SNN observes the pulse for exactly one
step. Waiting roughly one `--step-interval-ms` interval after the pulse
before sending the reset is a **best-effort** mitigation, not a guarantee —
the interval between drains also includes per-tick processing time (safety
checks, the network step itself, metrics/dashboard work), so a reset timed
to exactly one interval can still land in the same drain as the pulse under
load or scheduling jitter.

Request:

```json
{"type": "Stimuli", "values": [0.5, -0.2, 1.0, 0.0]}
```

- `values`: array of numbers (parsed as `f32`). Required, must be a JSON
  array or the message is ignored entirely (no partial apply).
- Each element is clamped to `[-1.0, 1.0]` before being applied. **Only the
  magnitude reaches the network**: the pinned `neuromod` 0.4.0
  `SpikingNetwork::step` applies `.abs()` to every stimulus value before
  using it, so `-0.2` and `0.2` drive the network identically. The sign has
  no inhibitory effect; the effective input range is `[0.0, 1.0]`.
- At most `N` values are applied, where `N` is the configured channel count
  (`--num-channels` / `THALAMIC_NUM_CHANNELS`, default `16`). Extra values
  beyond `N` are ignored; fewer values than `N` leave the remaining channels
  unchanged (not reset to zero) — see the persistence note above.
- A value that isn't a JSON number is treated as `0.0` before clamping.
- No reply is sent.

### `LearningReward`

Applies a reward/stress delta to the neuromodulator state.

Request:

```json
{"type": "LearningReward", "dopamine_delta": 0.3, "cortisol_delta": 0.1}
```

- `dopamine_delta`: number (`f32`), default `0.0` if absent or not a number.
- `cortisol_delta`: number (`f32`), default `0.0` if absent or not a number.
- Both deltas are clamped to a minimum of `0.0` (`.max(0.0)`) before being
  applied — **negative deltas are silently dropped and have no effect** on
  the corresponding modulator. This message type currently applies only
  positive reward/stress; use it to add dopamine/cortisol, not subtract.
- The delta is added to the current level and the result is capped at an
  upper bound of `1.0` (`neuromod`'s `add_reward`/`add_stress` apply
  `(current + delta).min(1.0)`): a large or cumulative delta can saturate
  dopamine/cortisol at `1.0` rather than being fully reflected.
- **Levels decay every SNN step, independent of this message.** The main
  loop calls `modulators.decay()` once per step (after applying any
  `LearningReward` received that step), which multiplies dopamine by
  `0.95` and cortisol by `0.90` (`neuromod`'s per-step decay constants). A
  level set by `LearningReward` is at its applied value only until the next
  step; a `GetNeuroState` reply reflects whatever decay has occurred since,
  so its value depends on step timing (`--step-interval-ms`), not just the
  accumulated deltas.
- No reply is sent.

### `GetNeuroState`

Queries the current neuromodulator and spike state. This is the only message
type that produces a reply.

Request:

```json
{"type": "GetNeuroState"}
```

Reply (sent back to the requester's source address as a single UDP
datagram, JSON-encoded):

```json
{
  "dopamine": 0.42,
  "cortisol": 0.10,
  "acetylcholine": 0.0,
  "lif_spike_count": 7
}
```

| Field              | Type    | Description                                            |
|--------------------|---------|----------------------------------------------------------|
| `dopamine`         | number  | Current dopamine level (`f32`)                          |
| `cortisol`         | number  | Current cortisol level (`f32`)                          |
| `acetylcholine`    | number  | Current acetylcholine level (`f32`). **Always `0.0` in the current relay**: raising it requires `neuromod`'s `boost_focus`, which nothing in `src/main.rs` calls, and decay never increases it — so this field is fixed at its default until a future relay version wires up a focus signal. |
| `lif_spike_count`  | integer | LIF (Leaky Integrate-and-Fire) spike count from the most recently completed SNN step |

If the reply fails to send (e.g. the source address is unreachable), the
failure is silently ignored — the relay does not retry or log.

## `--num-channels` interaction

`--num-channels` / `THALAMIC_NUM_CHANNELS` (default `16`, must be `>= 1`) sets
the size of the stimuli vector applied by `Stimuli` messages. It is fixed at
process startup — there is no message to change it at runtime. Clients should
size their `values` array to match the relay's configured channel count;
sending more or fewer is safe (see truncation/partial-apply behavior above)
but any values beyond the configured count are silently discarded.

## Examples

Using `netcat` (`nc`) against the default address:

```bash
echo -n '{"type":"Stimuli","values":[1.0,-1.0,0.5,0.0]}' | nc -u -w1 127.0.0.1 9898
echo -n '{"type":"LearningReward","dopamine_delta":0.3,"cortisol_delta":0.1}' | nc -u -w1 127.0.0.1 9898
echo -n '{"type":"GetNeuroState"}' | nc -u -w1 127.0.0.1 9898
```

Note `-n` (no trailing newline) — kept for a clean single-object datagram, per
the message framing note above.
