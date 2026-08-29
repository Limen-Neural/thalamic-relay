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
- **Message framing**: one JSON object per UDP datagram. Messages are
  **newline-free** — do not append `\n` or otherwise batch multiple JSON
  objects in a single datagram; only the first parses and any trailing bytes
  are part of that same JSON payload, not a second message.
- **Encoding**: UTF-8 text. Non-UTF-8 datagrams are silently dropped.
- **Max datagram size read**: 4096 bytes per receive; larger datagrams are
  truncated to that buffer size by the OS/socket read and will typically fail
  to parse as JSON.
- The relay drains all pending datagrams each step-loop tick (non-blocking
  socket) and processes them in the order received.

## Error behavior

The relay is intentionally lenient and never panics on malformed input:

- A datagram that is not valid UTF-8 is dropped.
- A datagram that is not valid JSON is dropped.
- A JSON object with a `"type"` that isn't a recognized value (or has no
  `"type"` field at all) is dropped.
- A recognized message with missing/malformed fields (wrong JSON type, out of
  range, etc.) is dropped for the fields that fail; see per-message notes
  below.
- **No error replies are sent.** The only message type that sends any
  response is `GetNeuroState`. All other messages are fire-and-forget: there
  is no ack, and no error is returned to the sender on failure.

## Message types

All requests are JSON objects with a `"type"` field selecting one of the
following.

### `Stimuli`

Drives the SNN's input channels for the next step.

Request:

```json
{"type": "Stimuli", "values": [0.5, -0.2, 1.0, 0.0]}
```

- `values`: array of numbers (parsed as `f32`). Required, must be a JSON
  array or the message is ignored entirely (no partial apply).
- Each element is clamped to `[-1.0, 1.0]` before being applied.
- At most `N` values are applied, where `N` is the configured channel count
  (`--num-channels` / `THALAMIC_NUM_CHANNELS`, default `16`). Extra values
  beyond `N` are ignored; fewer values than `N` leave the remaining channels
  unchanged (not reset to zero).
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
  "acetylcholine": 0.85,
  "lif_spike_count": 7
}
```

| Field              | Type    | Description                                            |
|--------------------|---------|----------------------------------------------------------|
| `dopamine`         | number  | Current dopamine level (`f32`)                          |
| `cortisol`         | number  | Current cortisol level (`f32`)                          |
| `acetylcholine`    | number  | Current acetylcholine level (`f32`)                     |
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

Note `-n` (no trailing newline) — see the newline-free framing note above.
