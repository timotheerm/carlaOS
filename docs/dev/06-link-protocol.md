# 06 · The link protocol (Ally ↔ desktop)

This chapter describes, end to end, how an Ally (the handheld running the shell, "carlaos-shell")
talks to a desktop (the companion, "carla-companion", shipped as *CarlaOS Desktop Host*). It is
written from the code; where the code does not settle a question the text says **UNVERIFIED:**
and what would confirm it.

Code map (all paths repo-relative):

| Side | Where |
| --- | --- |
| Shared wire types, constants (`PORT`, `Hello`, `Status`, `RemoteGame`, ...) | `crates/core/src/link/mod.rs` |
| Signing and replay window | `crates/core/src/link/sign.rs` |
| Linking (SPAKE2, code, approve flow) | `crates/core/src/link/enrol.rs` |
| The Ally's HTTP client (`Link`, `enrol`, `enrol_ask`, `hello`) | `crates/core/src/link/client.rs` |
| LAN discovery and the UDP echo responder | `crates/core/src/link/discover.rs` |
| Measuring: echo/burst, quality levels | `crates/core/src/link/probe.rs`, `quality.rs`, `health.rs` |
| Wake-on-LAN, address ordering | `crates/core/src/link/wake.rs` |
| The desktop's server (routes, auth, worker pool) | `crates/companion/src/server.rs` |
| The desktop's local dashboard (`/local/*`) | `crates/companion/src/dashboard.rs` |
| Listener start-up (TCP and UDP bind) | `crates/companion/src/main.rs` `serve` |
| How the shell uses the client | `apps/shell/src-tauri/src/hosts.rs`, `installs.rs`, `savesync.rs`, `remote.rs` |

Product-level behaviour (what a player sees) is in [STREAMING.md](../STREAMING.md),
[SAVE-SYNC.md](../SAVE-SYNC.md), [REMOTE-ADMIN.md](../REMOTE-ADMIN.md) and
[DASHBOARD.md](../DASHBOARD.md). Overall architecture: [01](01-architecture-and-runtime.md); the
desktop host in depth: [03](03-desktop-host.md); the Ally's native side: [04](04-ally-native.md);
recipes: [09](09-how-to-recipes.md).

---

## 1. Principles

1. **The Ally always initiates.** The desktop never connects out to the Ally. The Ally "listens"
   only in the sense that it long-polls (`GET /v1/admin/next`, section 8). Nothing has to be opened
   in the Ally's firewall.
2. **Requests and replies are signed, not encrypted.** Every request after linking carries an
   HMAC-SHA256 over method, path, time, nonce and body; every reply is HMAC-signed too and bound
   to the request's nonce. Between locations the traffic is expected to ride Tailscale, which
   encrypts it (module comment in `link/mod.rs`). Plain HTTP on the LAN exposes game names,
   status and save files to a sniffer. **UNVERIFIED:** whether any additional transport
   protection is planned (check `knowledge/decisions/`).
3. **One shared key per (Ally, desktop) pair**, derived while linking from a SPAKE2 exchange. The
   key is never sent.
4. **Default deny.** Anything not in the route table is a signed `404`; unknown/unsigned requests
   get `401`.
5. **The wire types live in `carla-core`** and both programs compile the same structs. Compatibility
   between versions is therefore a property of `serde` attributes (section 11), not of a schema
   file.

---

## 2. Transport

| Aspect | Value |
| --- | --- |
| TCP | HTTP/1.1, plain (no TLS). Port `carla_core::link::PORT` = **47600** by default. |
| UDP | The **same port number** (47600) on the companion, bound separately (`UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))` in `crates/companion/src/main.rs` `serve`). Used for discovery, echo and burst. |
| Port config | The companion's `Config.port` (`crates/companion/src/files.rs`) decides what it binds. The Ally stores the port per desktop in `HostRow.port` (`crates/core/src/library/mod.rs`); `PORT` is the default used when typing an address. |
| Server | `tiny_http` with 8 worker threads (`WORKERS`), see section 13. IPv4 only (`Ipv4Addr::UNSPECIFIED`). |
| Client | `ureq` 3 (`crates/core/src/link/client.rs` `agent`): global timeout per request, `http_status_as_error(false)`, so any HTTP status arrives as a reply, and only a connect/IO failure is an `Err`. |
| Bodies | JSON (`content-type: application/json`) for requests that have a body. A few replies are binary (`/v1/cover/..` pictures, `/v1/probe/{bytes}` filler, `/v1/saves/{key}/read` file contents). |
| Addressing | An address string plus port. IPv6 literals are bracketed (`client.rs` `url`). Discovery and the Ally's address ordering are IPv4-oriented (`wake::usable` drops IPv6). |
| Firewall | `carla-companion firewall` (`crates/companion/src/setup.rs`, `open_firewall`) adds inbound rules for the companion program on TCP and UDP with `remoteip=localsubnet,100.64.0.0/10` (home subnet and Tailscale). This is the only network configuration the repo performs for the link. |

Sunshine (the game-stream server) and Moonlight (the client) are **separate** programs with their
own ports (`47984`, `47989` and more); `client::SUNSHINE_PORTS` lists the two the Ally probes with
`client::port_open` to tell whether Sunshine is reachable through the firewall. The CarlaOS link
only coordinates them (prepare a game, hand over a pairing PIN).

---

## 3. Discovery (unauthenticated, advisory)

Discovery only saves the administrator from typing an address. Nothing in it is trusted; trust comes
from the code or approval afterwards.

**UDP question/answer** (`crates/core/src/link/discover.rs`):

* The Ally calls `discover::search(wait)`: it binds an ephemeral UDP socket with broadcast enabled
  and sends the 11-byte payload `QUESTION = b"carlaos? v1"` to `255.255.255.255:47600`, again every
  second, reading with a 200 ms timeout for the duration `wait` (the shell uses 2.5 s in
  `hosts.rs`).
* The companion runs `discover::answer(&socket, &|| server.hello())` in a thread. For each datagram
  exactly equal to `QUESTION` it replies, to the sender, with the JSON of `Hello` (same struct as
  `GET /v1/hello`). Echo packets are answered on the same socket (section 10).
* Results are keyed by `Hello.id` (a stable desktop id chosen at set-up); `Found.address` is the
  UDP source IP of the answer.
* Subnet-directed broadcast is not used: `255.255.255.255` only reaches the local segment, so
  discovery does not cross to a Tailscale peer or another subnet. For those the administrator
  types the address.

**Hello** (also served over TCP, unsigned):

```json
{ "name": "HOME PC", "id": "<stable id>", "version": "0.1.0", "api_version": 1, "approve": false }
```

`approve` is true exactly while "linking is open" on the desktop, i.e. while a code in
`enrol.json` is unexpired and has tries left (`Files::linking_until`). Anyone on the network can
learn name, id, version and whether linking is open.

---

## 4. Linking ("enrolment")

Two ways to link. Both run the same SPAKE2 password-authenticated key exchange
(`crates/core/src/link/enrol.rs`, `spake2::Ed25519Group`), with fixed identities `carlaos-ally` and
`carlaos-desktop`, and end by both sides holding the same 32-byte device key. The differences are
the "password" and a human check.

### 4.1 Constants (`enrol.rs`)

| Constant | Value | Meaning |
| --- | --- | --- |
| `CODE_LENGTH` | 6 | digits of the code |
| `CODE_LIFETIME` | 600 s | how long a shown code (and the open linking window) lasts |
| `CODE_TRIES` | 5 | uses of one code before it is thrown away. Each `enrol/start` and each `enrol/ask` uses one, right or wrong |
| `APPROVE` | `"carlaos-approve-v1"` | the (non-secret) SPAKE2 password when linking by approval |
| `APPROVE_LIFETIME` | 180 s | how long an approval request may wait for a person at the desktop |
| `WAITING` | `"Waiting for someone at the desktop to allow it."` | the `409` message while nobody has answered; the client matches this text |
| `EXCHANGE_AGE` (`server.rs`) | 60 s | how long a code-based exchange between `start` and `finish` stays valid |

Derived values (all `HMAC-SHA256(spake_key, "carlaos-enrol-v1:" + label + ":" + transcript)`, where
`transcript = hex(ally_msg) + ":" + hex(desktop_msg)`):

| Label | Use |
| --- | --- |
| `proof-ally` | proof the Ally holds the key (sent in `EnrolFinish.proof`) |
| `proof-desktop` | proof the desktop holds it (returned in `EnrolDone.proof`) |
| `check` | the four digits shown on both screens in the approve flow: first 4 bytes big-endian, `% 10000`, zero-padded |
| `device-key` | the 32-byte key that signs everything afterwards |

The device id is chosen by the desktop: `link::random_hex(8)` (16 hex chars) in `Files::add_device`.

### 4.2 Flow A: code typed on the Ally

Precondition: the administrator opened linking at the desktop (`Files::new_code`: writes
`enrol.json` with `code`, `expires = now + 600`, `tries = 0`; done by the host's first-run window or
the dashboard's `POST /local/code`, section 9).

| Step | Ally (`client::enrol`) | Desktop (`Server::enrol_start` / `enrol_finish`) |
| --- | --- | --- |
| 0 | Reject if code is not 6 digits (`valid_code`) | |
| 1 | `GET /v1/hello`; fail if `hello.api_version != API_VERSION` ("runs a different version of CarlaOS. Update both.") | |
| 2 | `Exchange::start(Side::Ally, code)`; `POST /v1/enrol/start` `{device_name, message}` | Name must be 1..64 chars after trim (else `400`). `Files::use_code(now)` consumes a try; none live: `403 "There is no code to enter right now..."`. `Exchange::start(Side::Desktop, code)`, `finish(start.message)` (bad hex/point: `400`). Stores `Waiting{shared, device_name, started}` under a fresh `enrol_id = random_hex(16)` (drops stale entries older than 60 s). Replies `EnrolOffer{enrol_id, message}` |
| 3 | `exchange.finish(offer.message)`. **A wrong code still "succeeds" here**: SPAKE2 yields unrelated keys, it does not error. Sends `POST /v1/enrol/finish` `{enrol_id, proof: shared.proof(Ally)}` | Removes the `Waiting` entry (so a failed finish cannot be retried; the Ally must start again, using another try). Checks freshness (`403 "That took too long. Start again."`). `proof_ok(Ally, proof)` else `403 "That is not the code on the desktop."`. On success: `Files::add_device(name, device_key)` (key sealed with `secret::seal` into `devices.json`), logs "A device was linked", `clear_code()` (so a successful link closes linking), replies `EnrolDone{device_id, proof: proof(Desktop)}` |
| 4 | `shared.proof_ok(Desktop, done.proof)` else error "That is not the code on the desktop." The Ally then keeps `Link{address, port, device_id, key: shared.device_key()}` | |

Security properties encoded here: an eavesdropper never sees the code; an active guesser gets one
guess per consumed try (5 per code); the Ally verifies the desktop's proof, so an impostor that did
not know the code cannot be "linked to".

### 4.3 Flow B: approval at the desktop (no code typed)

Used by the shell's "Add a desktop" (`hosts_link_ask` then `hosts_link_wait`, both gated by the
Ally's administrator PIN via `admin.require()`). The desktop only accepts it while linking is
open (a live code exists; the code itself is not shown to the Ally).

The SPAKE2 password is the public constant `APPROVE`, so the protection is a **commit-then-reveal**
plus a **human comparison of four digits**:

| Step | Ally (`client::enrol_ask`, `Asking::finish`) | Desktop |
| --- | --- | --- |
| 1 | `GET /v1/hello` (version check) | |
| 2 | `Exchange::start(Ally, APPROVE)`; computes `commit = sha256("carlaos-commit-v1:" + message)`; `POST /v1/enrol/ask` `{device_name, commit}` | Validates name (1..64, no control chars) and `commit` (64 hex chars) else `400`. Refuses `409 "Another device is asking..."` if another approval is pending (one at a time). `use_code` consumes a try (`403 "Linking is not open on the desktop right now..."`). Creates its own `Exchange(Desktop, APPROVE)`, stores `Waiting{shared: None, approve: Some{exchange, commit, from}}`. Replies `EnrolOffer{enrol_id, message}` (the desktop's message goes out **before** the Ally's is revealed) |
| 3 | `exchange.finish(offer.message)`, then derives `check = shared.check()`. `POST /v1/enrol/reveal` `{enrol_id, message}` | Entry must exist and be fresh (180 s) else `403 "That took too long. Start again."`. Takes the stored exchange (second reveal: `400`). If `sha256("carlaos-commit-v1:"+message) != commit` the entry is deleted and the reply is `400`; so the Ally cannot choose its message after seeing the desktop's. Otherwise derives the same `shared`, writes `enrol-ask.json` (`Asking{enrol_id, device_name, from, check, at}`) and replies `{"asking": true}` |
| 4 | Returns `Asking` carrying `check`; the shell shows it. | The person at the desktop sees the name, source address and `check` (the companion's console window `attend()` in `main.rs`, and the dashboard `Overview.asking`). They compare it with the Ally's screen and choose allow/refuse: `Files::answer_asking(enrol_id, allow, now)` writes `enrol-answer.json` |
| 5 | Polls `Asking::finish()` about once a second (`hosts_link_wait`, up to `APPROVE_LIFETIME`): `POST /v1/enrol/finish` `{enrol_id, proof}`. A `Host` error whose text equals `enrol::WAITING` becomes `Ok(None)` ("not yet") | `enrol_finish`: pending approval still waiting: entry re-inserted, reply `409 WAITING`. Refused: `clear_code()`, `403 "It was not allowed at the desktop."`. Allowed: verify `proof`, `add_device`, `clear_code`, reply `EnrolDone` as in flow A |
| 6 | `proof_ok(Desktop, ...)` else `Error::Untrusted`. Keeps the `Link` | |

Properties: a man in the middle ends up with a different key per side, so the two `check` numbers
differ and the human refuses; neither side can steer the number because the Ally committed before
seeing the desktop's message. Tests: `an_ally_is_linked_by_being_allowed_at_the_desktop_without_a_code`,
`a_link_that_is_refused_at_the_desktop_links_nothing`,
`a_message_that_was_not_committed_to_is_refused` in `crates/companion/src/server.rs`.

Caveats visible in the code:

* A poll of `enrol/finish` **before** `enrol/reveal` has completed finds `shared == None` and
  yields `403 "That took too long"`, and the entry has already been removed by the `remove` at the
  top of `enrol_finish`. Clients must reveal first (the shipped client does).
* The 409 for "waiting" and the 409 for "another device is asking" and "clock disagreement" all
  arrive as `Error::Host(text)`; the client distinguishes only by text (`enrol::WAITING`).

### 4.4 After linking

* The desktop persists `Device{id, name, key(sealed hex), added}` in `devices.json`
  (`Files::devices`). Forgetting: `Files::forget_device(id)` (CLI `carla-companion forget ID`,
  dashboard `POST /local/forget`); it also deletes `ally-seen-<id>.json` and `ally-pulse-<id>.json`.
* The Ally persists a `HostRow` in the SQLite `hosts` table with the key sealed via
  `carla_core::secret::seal` (shell: `finish_link` in `hosts.rs`). `hosts_remove` just deletes the
  row; **the Ally never tells the desktop it was forgotten** (there is no route for it), so the
  desktop keeps the device until its administrator forgets it.
* The shell then pairs Moonlight with Sunshine (`pair_moonlight` -> `POST /v1/stream/pair`),
  fetches the game list (`GET /v1/games`), and measures the connection (section 10).

---

## 5. Request signing

Defined in `crates/core/src/link/sign.rs`; used by `client::Link::ask_once` (signer) and
`Server::trusted` (verifier).

### 5.1 Headers (names are lower-case constants in `sign.rs`)

| Header | Constant | Value |
| --- | --- | --- |
| `x-carla-device` | `DEVICE` | the desktop-assigned device id (16 hex) |
| `x-carla-time` | `TIME` | Unix seconds on the Ally's clock |
| `x-carla-nonce` | `NONCE` | 32 hex chars (`random_hex(16)`); the server accepts 16..=64 chars |
| `x-carla-sign` | `SIGN` | lower-hex HMAC-SHA256 |

The same header `x-carla-sign` carries the signature on **replies** from the desktop.

### 5.2 What is signed

```
request  = HMAC_SHA256(device_key,
  "carla-v1\n" + METHOD_UPPER + "\n" + path + "\n" + time + "\n" + nonce + "\n" + hex(sha256(body)))

reply    = HMAC_SHA256(device_key,
  "carla-v1-reply\n" + nonce + "\n" + status + "\n" + hex(sha256(body)))
```

* `path` is the request path **without query**; the client never uses a query. IDs such as
  `steam:20` appear raw in the path (`/v1/cover/steam:20`, no percent-encoding); `tiny_http` hands
  the raw URL to the server and the server compares the same raw string.
  **UNVERIFIED:** behaviour for ids containing characters that need escaping (`emu:<console>-<key>`
  ids are built from a console id and a hash; confirm that console ids cannot contain spaces or `?`
  by reading `Console` validation in `crates/core/src/library/emulation.rs` `checked`).
* `body` is the exact bytes sent (empty for `GET`). A save write body is `<json line>\n<raw file bytes>`
  and is signed as a whole.
* The reply signature covers nonce, status and body but **not** content-type or other headers.

### 5.3 Verification order on the desktop (`Server::handle` -> `trusted` -> `signed`)

1. If the route is one of the five unsigned routes (`GET /v1/hello`, `POST /v1/enrol/{start,finish,ask,reveal}`)
   it is served without signature checks, and its reply is **not signed** (`Reply.sign = None`).
2. Otherwise `trusted()`: device, nonce, sign and time headers must all be present; `time`
   must parse as `i64`; the device must be in `devices.json` and its sealed key must open
   (`Device::key()` can fail if the file came from another Windows account); then
   `sign::request_ok` (constant-time `verify_slice`). Any failure: **`401`**, unsigned body
   `{"error":"This device is not linked to the desktop."}`.
3. A genuine signature is then checked by `Replay::fresh(nonce, time, now)`: `|now - time| <= WINDOW`
   (`WINDOW = 300` s), nonce length 16..=64, and the nonce not seen within the window.
   Failure: **`409`** `"The Ally and the desktop disagree about the time. Check the clock on both."`,
   and this refusal **is signed** (the key is known) so the Ally can trust the message. The
   companion logs "Refused a request of <id>: it is too old, too new or repeated...".
4. On success `signed()` dispatches on (method, path). The reply is signed with the same key and
   the request's nonce.

Replay set: `Server.replay: Mutex<Replay>` is in memory only. **UNVERIFIED:** nothing persists it,
so after a companion restart a request captured within the last 300 s could be accepted once more;
confirm by reading `Replay` (no persistence) and testing a restart.

### 5.4 What the client does with an answer (`Link::ask_once`)

| Observed | Result |
| --- | --- |
| connect/IO failure or timeout | `Error::Unreachable` |
| status `401` | `Error::Refused`: the desktop no longer accepts this Ally (forgotten, or key changed). Unsigned by design. |
| any other status whose `x-carla-sign` is missing or does not verify against `(key, request nonce, status, body)` | `Error::Untrusted`: not the desktop we linked (address now belongs to another machine, or a man in the middle) |
| signed `200` | body returned |
| signed non-200 | `Error::Host(<Problem.error>)` via `problem(body)`, or the status and body to the caller for `ask_status` users |

Errors are rendered by `Display for Error` into the sentences the UI shows. Reply bodies are
capped (`LARGEST_REPLY` = `sync::LARGEST_FILE + 1 MiB`, 33 MiB).

**Quirk worth knowing:** the desktop's own pre-routing refusals (`plain_reply`: `408` body timeout,
`400` unreadable request, `503` "busy with other requests") carry **no signature**, so a linked Ally
receives them as `Error::Untrusted`, not as `Error::Host("... busy ...")`. **UNVERIFIED** whether this
is intended; it is visible in `work()` (`plain_reply` has `sign: None`) and `ask_once` (unsigned,
non-401 -> `Untrusted`).

### 5.5 Observer

`client::set_observer(fn(&Call))` registers a process-wide callback (the shell registers
`conn_log::observe`). It is told, after each signed request, `{method, path (query stripped and the
save-folder key replaced by "*"), status, error ("unreachable"|"refused"|"untrusted"|"host"),
elapsed_ms, route ("local"|"remote" by `wake::is_tailscale`)}`. Never the address, body or headers.
The callback runs on the calling thread, so it must be quick.

---

## 6. Complete route table

"Signed" = `x-carla-*` headers required and verified (section 5). "Open" = no authentication.
Timeouts are the **client-side** total timeouts (`client.rs`: `QUICK` = 3 s, `SLOW` = 40 s,
`PROBE` = 4 s, `COVER` = 6 s; `open()` calls use `QUICK*2` = 6 s). Every signed route can also
return the generic errors: `401`, `409` (clock), `404` (`"The companion does not know that request."`
for any unmapped signed route), `503` (too many long requests, unsigned), `408`/`400` (body, unsigned).

| # | Method + path | Auth | Request body | Success reply | Purpose | Client call | Timeout | Notable errors |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `GET /v1/hello` | open | - | `Hello` | Identify, version, "linking open?" | `client::hello` | 6 s | - |
| 2 | `POST /v1/enrol/start` | open | `EnrolStart{device_name, message}` | `EnrolOffer{enrol_id, message}` | Code-flow step 1 | `client::enrol` | 6 s | `400` name/message, `403` no live code |
| 3 | `POST /v1/enrol/finish` | open | `EnrolFinish{enrol_id, proof}` | `EnrolDone{device_id, proof}` | Finish either flow | `client::enrol`, `Asking::finish` | 6 s | `403` took too long / wrong code / refused, `409 WAITING` |
| 4 | `POST /v1/enrol/ask` | open | `EnrolAsk{device_name, commit}` | `EnrolOffer` | Approve-flow step 1 | `client::enrol_ask` | 6 s | `400`, `403` linking closed, `409` another device asking |
| 5 | `POST /v1/enrol/reveal` | open | `EnrolReveal{enrol_id, message}` | `{"asking":true}` | Approve-flow step 2 | `client::enrol_ask` | 6 s | `403` took too long, `400` commit mismatch |
| 6 | `GET /v1/status` | signed | - | `Status` | Reachability + state (`ready`/`busy`/`not_ready`), addresses, MACs, graphics | `Link::status` | 3 s | - |
| 7 | `GET /v1/games` | signed | - | `Vec<RemoteGame>` | Installed games (Steam/Epic/GOG/emulated/manual) | `Link::games` | 40 s | - |
| 8 | `GET /v1/library` | signed | - | `RemoteLibrary{games, free_bytes, problem}` | Steam library games not installed there | `Link::library` | 40 s | - |
| 9 | `POST /v1/install` | signed | `Install{game_id}` | `Installing{asked_store}` | Start a Steam install there | `Link::install` | 80 s | `404` not in library, `409` a game is running, `503` Steam |
| 10 | `GET /v1/downloads` | signed | - | `Vec<Download>` | What stores there are fetching | `Link::downloads` | 6 s | - |
| 11 | `GET /v1/cover/{game_id}` | signed | - | image bytes (`image/jpeg|png|webp`) | Cover picture | `Link::cover` | 6 s | `404` |
| 12 | `GET /v1/achievements/{game_id}` | signed | - | `Vec<Achievement>` | Steam achievements read on the desktop (empty for other stores) | `Link::achievements` | 3 s | `404` not installed |
| 13 | `GET /v1/games/update/{game_id}` | signed | - | `Option<Update>` (JSON `null` when none) | Fresh "store has an update" check just before starting | `Link::pending_update` | 3 s | an older desktop answers `404` (client treats as "nothing known" at call site) |
| 14 | `GET /v1/probe/{bytes}` | signed | - | `bytes` of `application/octet-stream` filler | Download timing | `Link::probe` | 4 s | `400` if `bytes > 2 MiB` or not a number |
| 15 | `POST /v1/probe/burst` | signed | `BurstAsk{port, kbps, ms}` | `BurstPlan{packets, ms}` | Ask for a UDP burst (section 10) | `Link::burst` | 4 s | `400` too much, `409` stream/other burst running, `503` no UDP socket |
| 16 | `POST /v1/link/pulse` | signed | `Pulse` | `{"received":true}` | Ally tells the desktop its state and the latest `Health` | `Link::pulse` | 3 s | `500` could not store |
| 17 | `POST /v1/stream/fit` | signed | `FitAsk{screen: Mode}` | `ScreenFit{virtual_screen, mode}` | What screen a stream of that size would get | `Link::fit` | 3 s | - |
| 18 | `POST /v1/stream/prepare` | signed | `Prepare{game_id, screen?}` | `Status` | Tell the desktop which game the next Moonlight connection is for; it writes `pending` (read by its launcher) and may switch/create the screen | `Link::prepare`, `prepare_for` | 40 s | `404` not installed, `409` another game running, `503` Sunshine, `500` |
| 19 | `POST /v1/stream/pair` | signed | `Pair{pin, name}` | `{"paired":true}` | Hand over Moonlight's 4-digit PIN to Sunshine | `Link::pair` | 40 s | `400` PIN not 4 digits, `503` |
| 20 | `POST /v1/stream/reveal` | signed | `{}` | `{"revealed": bool}` | Ask the launcher to remove its loading curtain | `Link::reveal` | 3 s | - |
| 21 | `POST /v1/stream/end` | signed | `{}` | `{"ended": bool}` | Ask the launcher to close the running game | `Link::end_game` | 3 s | - |
| 22 | `POST /v1/power/sleep` | signed | `{}` | `{"sleeping":true}` | Sleep the desktop | `Link::sleep` | 3 s | `409` a game runs, `503` |
| 23 | `POST /v1/update` | signed | `{}` | `Updating{updating, version}` | Update the companion; it restarts | `Link::update` | 40 s | `409` a game runs, `503` |
| 24 | `GET /v1/admin/next` | signed | - | `Option<remote::Command>` (`null` if none) | Long poll for an administrator command (section 8) | `Link::admin_next` | 40 s | - |
| 25 | `POST /v1/admin/done` | signed | `remote::Done` | `{"received":true}` | Return a command's result | `Link::admin_done` | 6 s | `500` |
| 26 | `POST /v1/saves/find` | signed | `SavesFind{game_id, title, place?}` | `Option<SavesFound>` (`null` = no folder there) | Locate the desktop's save folder; may note a suggestion | `Link::saves`, `saves_placed` | 40 s | `409` game running, `503` |
| 27 | `POST /v1/saves/{key}/read` | signed | `SaveFile{path}` | file bytes | Read one save file | `RemoteSaves` `Side::read` | 40 s | `404` |
| 28 | `POST /v1/saves/{key}/write` | signed | `<SaveFile json>\n<bytes>` | `{"done":true}` | Write one save file (with mtime) | `Side::write` | 40 s | `400` no newline, `409` refused path/other |
| 29 | `POST /v1/saves/{key}/remove` | signed | `SaveFile{path}` | `{"done":true}` | Remove one file | `Side::remove` | 40 s | `409` |
| 30 | `POST /v1/saves/{key}/protect` | signed | `{}` | `{"done":true}` | Take a safe copy first | `Side::protect` | 40 s | `503` |
| 31 | `POST /v1/saves/{key}/retire` | signed | `sync::Manifest` (Ally's) | `Retired{files, bytes, skipped, backup}` | Desktop clears its copy after proving an identical Ally copy | `RemoteSaves::retire` | 40 s | `403` not flagged, `400` unsound manifest, `409` mismatch/ running, `503` backup failed |

Server-side dispatch is the `match (req.method, req.path)` in `Server::signed`
(`crates/companion/src/server.rs`); `/v1/saves/` and the four prefix routes (`cover`, `achievements`,
`games/update`, `probe`) use `starts_with`, in the order shown in the code (`/v1/saves/find` is
matched before the generic `/v1/saves/` prefix; keep that order when adding routes).

Notes per route:

* **`status`**: `State` is derived: Sunshine unhealthy -> `not_ready` (with `problem`), else a game
  running -> `busy`, else `ready`. Sunshine health is cached for 15 s (`HEALTH_AGE`); a concurrent
  check returns the previous answer instead of waiting. `addresses`/`macs` are read afresh each time
  (Tailscale and Wi-Fi come and go).
* **`games`**: Steam/Epic/GOG scan results and manual games and emulated games, cached 60 s
  (`SCAN_AGE`); `Found = (&'static str source, Scanned)`.
  `RemoteGame.id` is `"<source>:<source_id>"`, or `manual:<key>`, or `emu:<console>-<key>`.
* **`prepare`**: is the only route that records `Seen.prepared` for the dashboard (see `note_seen`);
  see [03](03-desktop-host.md) for the launcher side.
* **`install`**: only an id from the desktop's own offered list can be installed; the Ally cannot name
  a path or other store.
* **`saves/*`**: serialised under a static `SAVES` lock (all but `read`), refused while a game runs
  (`409` "still running..."), restricted to the folder the administrator mapped
  (`SaveFolder.key`), and `sync::inside` rejects traversal. `key` is not secret but is an
  opaque handle; unknown key -> `404 "<desktop> no longer keeps that game's saves in step."`.
* **`retire`**: the client maps statuses itself via `Link::ask_status`: `200 -> Done`,
  `403 -> NotFlagged`, `404 -> Unsupported` (an older desktop), `409 -> Kept(reason)`.

### 6.1 Request-size and concurrency limits on the desktop (`work()`)

| Case | Body limit |
| --- | --- |
| Default (anyone) | 64 KiB |
| Linked device, `POST /v1/saves/*/write` | `sync::LARGEST_FILE + 4096` (about 32 MiB) |
| Linked device, `POST /v1/saves/*/retire` | 1 MiB |
| Linked device, `POST /v1/admin/done` | 2 MiB |
| "Long" path from someone not trusted | 4096 bytes |

"Linked" here means only that the *claimed* `x-carla-device` id exists in `devices.json`
(`Server::knows`), evaluated **before** the signature check (a deliberate cheap pre-filter so big
bodies are not read from strangers). **UNVERIFIED (observation, not a confirmed vulnerability):**
a stranger who learns a device id can get a larger body read before being refused with `401`;
device ids are not secret but are only exchanged over this link.

Long paths (`takes_long`): `/v1/saves/*`, `/v1/admin/next`, `/v1/install`, `/v1/stream/pair`,
`/local/find`, `/local/games`. At most `LONG_AT_ONCE` = 4 of the 8 workers may be on them, and only
for a linked device or this PC; excess returns `503` (unsigned). Body arrival timeouts: base 30 s
(`BODY_WITHIN`), capped at 5 s for bodies up to 64 KiB, extended by 1 s per MiB up to 180 s for
large ones; sockets have 10 s read/write timeouts (`IO_WAIT`, applied on Windows via `setsockopt`
in `bind`; elsewhere only the body deadline applies). `bind` returns whether the timeouts took
effect and the companion logs a WARNING if not.

---

## 7. Error and status conventions

* Every non-success reply body is `Problem{ "error": "<sentence>" }` (`link::Problem`). The sentence
  is meant to be shown to a person as written; build such sentences, do not return codes.
* Status codes in use: `200` success; `400` malformed/ out of range; `401` unknown device or bad
  signature (unsigned); `403` forbidden by state (no code, refused, not flagged); `404` unknown route
  or object; `408` request body too slow; `409` conflict (a game is running, clock skew, another
  device asking, waiting); `500` the desktop could not write its own files; `503` a dependency is
  unavailable (Sunshine, Steam, no UDP socket, too many long requests).
* There is no machine-readable error code besides the HTTP status and, for one case, the exact text
  `enrol::WAITING`. Avoid adding more text-matching; prefer a status code or a typed field.

---

## 8. Admin long-poll (`/v1/admin/next`, `/v1/admin/done`)

Administering an Ally from the desktop (`carla-companion ally ...`, see [REMOTE-ADMIN.md](../REMOTE-ADMIN.md))
without opening any port on the Ally:

1. The desktop's CLI queues a `remote::Command{id, op, args}` for a device (`Files::ask_ally`).
2. The Ally's shell loop (`apps/shell/src-tauri/src/remote.rs`) calls `Link::admin_next()` on every
   desktop whose administrator allowed it (`HostRow.admin` / `Store::host_admin`). The server holds the
   request open (`Server::admin_hold`, sleeping 200 ms steps, calling `Files::ally_listened`) until a
   command arrives or the hold ends, then answers `null`. The client's 40 s timeout is longer than the
   hold, so the Ally immediately asks again.
3. The Ally executes the op (`carla_core::remote::catalogue` for catalogue ops, the shell for the rest;
   allowed names in `remote::OPS`) and replies with `POST /v1/admin/done` `Done{id, ok, output}`.
4. A parked `admin/next` holds a worker; that is why only 4 of 8 workers may be on long requests.

`Files::ally_listened` is also what makes an Ally appear "connected" on the dashboard even when idle.

---

## 9. The `/local` dashboard routes (loopback only, not part of the signed API)

Served from the same TCP port but by `Server::local` (`crates/companion/src/dashboard.rs`), called
from `work()` **only when the TCP peer address is loopback** (`request.remote_addr().ip().is_loopback()`).
It returns `Some(Reply)` for `/` and for paths starting with `/local/`; anything else falls through to
the signed API. A signed `/v1/...` request from loopback is therefore still signed-API traffic.

Checks, in order (`Server::local`):

1. Path must be `/` or start with `/local/`, else `None`.
2. **Host header**: `own_name(host)` must be `127.0.0.1` or `localhost` (case-insensitive,
   optional `:port`). Otherwise `403 "The dashboard only opens on this PC itself."` This blocks DNS
   rebinding (a web page elsewhere pointing a name of its own at 127.0.0.1).
3. `GET /` returns the HTML page with a per-process secret substituted for `__CARLA_TOKEN__`
   (`Server.token`). Response headers add `cache-control: no-store`, `x-frame-options: DENY` and a
   strict CSP (`default-src 'none'; ... connect-src 'self'; frame-ancestors 'none'`).
4. Every other route requires header **`x-carla-token`** equal to that token, else
   `403 "This page is out of date. Reload it."` Browsers will not let another origin set this custom
   header without a CORS preflight, which the server does not answer.
5. Then dispatch by (method, path):

| Method + path | Purpose | Notes |
| --- | --- | --- |
| `GET /local/overview` | The whole dashboard model: `link::health::Overview` (state, games running, addresses, `linking_until`, `asking`, `allies` with health) | not logged when 200 (`too_common`) |
| `GET /local/log` | newest lines of `host.log` | `LOG_LINES` limit |
| `GET /local/newer`, `POST /local/newer` | cached / forced check for a newer release | |
| `POST /local/code` | open linking: new 6-digit code | `Files::new_code` |
| `POST /local/code/hide` | close linking | `Files::clear_code` |
| `POST /local/forget` | forget a linked device `{id}` | |
| `POST /local/games` | rescan games now | long path |
| `POST /local/find` | look for programs that could be games | long path; only one at a time |
| `POST /local/add` | add one of the last found programs by id | body `{id}` only (`deny_unknown_fields`); revalidates the file stamp (`409` if changed) |
| `POST /local/update` | update the companion | same checks as `/v1/update` |
| `POST /local/end` | close the running game | |
| `POST /local/virtual/install` | install the virtual-screen driver (winget; Windows prompts) | takes no parameters (`400` if a body); one at a time |
| other | `404 "The dashboard does not know that request."` | |

The dashboard answers are never signed (`Reply.sign = None`). Tests that prove the checks:
`crates/companion/src/dashboard.rs` (`evil.example` Host, missing token, guessed token, `GET` vs
`POST`, parameters).

Adding a `/local` route: add the arm in `Server::local`, keep it behind the token, make state-changing
routes `POST`, take no free-form paths from the page (the page names ids from the last server-side
list, never paths), and add a test for wrong Host / missing token / wrong token.

---

## 10. Measuring the connection and choosing the picture

Goal: pick the video level (resolution/fps/bitrate) from what the path really carries, without
starting a stream.

### 10.1 What is measured (`crates/core/src/link/quality.rs`, `probe.rs`)

* **Web-request timing** over the signed link (`Link::probe(bytes)` = `GET /v1/probe/{bytes}`):
  `measure()` sends 4 empty probes (`/v1/probe/0`) for latency (minimum) and jitter (max - min),
  then a first 128 KiB download for a rate estimate, then, if the estimate leaves room, a second
  download sized for about 0.8 s (capped at `LARGEST_PROBE` = 2 MiB, and the server enforces the
  same cap with `400`).
* **UDP echo** (`probe::echo`): the Ally sends numbered 1200-byte packets (`PACKET`) with magic
  `b"carla~e1"` + sequence + total + deterministic pseudo-random filler to the companion's UDP port.
  The companion's `discover::answer` loop returns any datagram that `probe::is_echo` accepts
  (>= 16 and <= 1200 bytes, magic `ECHO`) **unchanged**, to anyone. Because the reply is exactly as
  large as the question it cannot amplify a flood. The Ally computes `Echo{sent, lost, best_us,
  median_us, p95_us, jitter_us}`; waits 500 ms for stragglers; accepts replies only from the target IP.
* **UDP burst** (`Link::burst`, `Depth::Full` only): the Ally binds a UDP socket, first sends 3 echo
  packets (opens its own firewall for the return path), then `POST /v1/probe/burst` `{port: <its
  socket's port>, kbps, ms}`. `probe::plan` limits: `0 < kbps <= MOST_KBPS` (150 000), `0 < ms <=
  LONGEST_MS` (1500), `port != 0`. The companion refuses while a stream is running (`409`) or another
  burst is in progress (`409`), needs its probe socket (`503`), answers `BurstPlan{packets, ms}`, and
  from a thread waits 200 ms then sends `packets` UDP packets (magic `b"carla~b1"`) evenly over `ms`
  **to the IP the signed request came from** and the requested port. The Ally counts arrivals
  (`receive_burst`) and returns `Burst{asked_kbps, got_kbps, sent, lost}`. A burst is only ever sent
  to a linked Ally's address because the request that triggers it is signed.
* `quality::survey(line, away, depth)` combines them (`Depth::Quick`: 20 echo packets; `Depth::Full`:
  100 echo packets and, if the download shows room, a one-second burst at
  `min(1.5 x heaviest picture, 90 % of measured kbps)`). `Link` implements the `quality::Line` trait
  (`download`, `echo`, `burst`), which is how tests substitute fake lines.

### 10.2 Choosing a level (`quality::choose`)

`Setting` is what the administrator picked per desktop (stored in `hosts.quality`): `Auto{smooth}` or
`Fixed(Level)`; names `auto`, `auto120`, `lightest`, `light`, `balanced`, `sharp`, `smooth`.

| `Level` | Picture (`Level::picture`) | needs (`needs_kbps` = kbps x 9/5) |
| --- | --- | --- |
| lightest | 1280x720 @ 30, 3 000 kbps | 5 400 |
| light | 1280x720 @ 60, 7 000 | 12 600 |
| balanced | 1920x1080 @ 60, 12 000 | 21 600 |
| sharp | 1920x1080 @ 60, 25 000 | 45 000 |
| smooth | 1920x1080 @ 120, 40 000 | 72 000 |

For `Auto`:

* No measurement, or a measurement with no `kbps`: `Sharp` at home, `Balanced` when `away`
  (Tailscale address): this is the behaviour from before measuring existed, kept for old desktops
  that cannot answer `/v1/probe`.
* Otherwise `top` starts at `Sharp` (or `Smooth` if `auto120`, not away, latency <= 15 ms, jitter <=
  4 ms and loss <= 0.5 %); jitter > 40 ms or loss > 2 % caps at `Balanced`; loss > 5 % caps at
  `Light`. The chosen level is the heaviest `<= top` whose `needs_kbps` is at most the measured
  capacity (`Measure::carries_kbps`, which accounts for burst results), else `Lightest` ("still
  tries").
* `Level::picture_on(screen)` reshapes the picture to the Ally's own screen and reduces to 60 fps
  when the screen is below 100 Hz.

`health::grade` turns a `Measure` + level into `Grade` (`Poor < Playable < Good < Excellent`) using
the worst of delay, jitter and loss thresholds, and `Health::of` builds the one value
(`Health{at, route, address, grade, summary, ping_us, best_us, p95_us, jitter_us, loss_permille,
link_kbps, burst, picture, codec, advice}`) that every screen shows. The Ally sends the latest
`Health` inside each `Pulse` (about every 45 s: `PULSE_EVERY` in `hosts.rs`; a pulse while a game
runs carries `health: None` and the desktop keeps the previous measurement; nothing is measured
during a stream).

### 10.3 Pulse and "connected"

`POST /v1/link/pulse` (`Pulse{version, moonlight, paired, playing, battery, charging, screen,
health}`) is stored in `ally-pulse-<id>.json`. An Ally counts as connected on the desktop's dashboard
if its last signed request or last `admin/next` was within `CONNECTED_FOR` = 90 s, or it is the Ally
that most recently prepared a stream and a stream is running (`Files::allies`).

---

## 11. Compatibility rules (how old and new peers behave)

Both programs are updated independently (each from the same GitHub releases, `crates/core/src/update.rs`),
so a new Ally will meet an old desktop and vice versa. The rules that keep this safe:

1. **Serde ignores unknown fields.** There is no `deny_unknown_fields` on any wire type in
   `carla-core` (the only use in the repo is the dashboard's private `Pick` body for `POST /local/add`). A newer peer
   adding a field never breaks an older reader.
2. **A field the older peer does not send must be optional on the reader**:
   `#[serde(default)]` (missing -> default) or `Option<T>`. Existing examples (all in
   `link/mod.rs` unless noted): `Hello.approve`; `Status.addresses/macs/graphics`;
   `RemoteGame.platform/emulated/update/description`; `Prepare.screen`;
   `SavesFind.place`; `SavesFound.place/keep_clean`; `SaveFile.modified`; `Retired.skipped`;
   `Pulse.battery/charging/screen` (`health.rs`); `remote::Command.args`.
3. **A field an older reader must not see is hidden when empty**:
   `#[serde(default, skip_serializing_if = "String::is_empty")]` on `RemoteGame.description`. The test
   `a_game_from_an_older_desktop_has_no_description_and_none_is_sent_when_empty` (in `link/mod.rs`)
   pins this.
4. **Never rename or retype a field**, or change an enum's `rename_all` string; add a new field.
   Enum variants added to a type that an *older* peer deserialises (`State`, `Grade`, `Route`, `Fix`
   `Outcome`, `Way`...) would fail to parse there: serde enums are strict. The one precedent for a
   safe fallback is `library::saveplace::SaveRoot`, which has `#[serde(other)] Unknown` so a newer
   desktop's root becomes "hint dropped" on an older Ally (it travels inside `SavesFind.place` and
   `SavesFound.place`). For any enum that crosses the link, copy that pattern or add a new optional
   field instead of a variant. **UNVERIFIED:** no other wire enum has a fallback; confirm by reading
   each `enum` in `link/` (`State`, `Grade`, `Route`, `Fix`, `Outcome`, `Way`, `Codec`, `Level`).
7. **JSON number/field types are fixed**: i64 Unix seconds, u64 byte counts, u32 kbps/us.
   Moving to floats or strings would break older readers.

### 11.1 Versioning

* `carla_core::API_VERSION: u32 = 1` ("Version of the companion HTTP API (`/v1/...`)") and
  `carla_core::VERSION` (crate version = release number stamped at build, see `docs/RELEASES.md`).
* `AppInfo{role: shell|companion, version, api_version, os, arch}` (`crates/core/src/lib.rs`
  `AppInfo::current`) is the generic self-description used by the programs' own info screens.
  **UNVERIFIED:** it is not on the wire in any route found; confirm with
  `grep -rn "AppInfo" crates apps`.
* `Hello{version, api_version}` is the only place a version is compared, and only at **link time**:
  `client::hello` rejects `api_version != API_VERSION`. After linking, version differences are
  tolerated: `Status.version` is just displayed, and the Ally offers "Update the desktop"
  (`Fix::Update`, `POST /v1/update`) in the connection check (`link/check.rs`).
  Consequence: bumping `API_VERSION` stops *new links* between old and new, but does not disconnect
  existing links. Prefer additive changes and keep `API_VERSION` unchanged. **UNVERIFIED:** any
  policy for what a bump is reserved for; search `knowledge/decisions/`.

---

## 12. Link-state cases (what each failure means and what the UI does)

| Situation | Wire evidence | Client `Error` | Typical UI reaction (`link/check.rs`, `hosts.rs`) |
| --- | --- | --- | --- |
| Desktop off, asleep, wrong network, firewall | connect/timeout | `Unreachable` | "It did not answer... probably asleep or off"; offer **Wake** if MACs are known (`wake::wake`: Wake-on-LAN magic packet to `255.255.255.255` ports 9 and 7, twice) |
| Desktop forgot this device, or was set up again, or the key cannot be opened | `401`, unsigned | `Refused` | "no longer accepts this Ally... needs a new code" with **Relink** (`Fix::Relink`), which in the shell only tells the user to open the Desktop Host and add the desktop again |
| Something else answers at that address (router reassigned it), or reply tampered | reply without a valid signature | `Untrusted` | "Something answered at its address, but not the desktop this Ally linked" with **Relink** |
| Clocks differ more than 300 s, or a nonce repeated | signed `409` with the clock sentence | `Host(text)` | message shown as is; fix the clock on both |
| A game is running / Sunshine missing | signed `409`/`503` with a sentence | `Host(text)` | message shown |
| Desktop busy (all workers) | unsigned `503` | `Untrusted` (see section 5.4 quirk) | shown as untrusted |
| Different `api_version` at link time | `Hello` | `Host("... runs a different version of CarlaOS. Update both.")` | link refused |

Address selection: `hosts::reach` (shell) asks **every** known address in parallel
(`wake::candidates(last, reported)`: the last working address first, then home-network addresses, then
Tailscale `100.64.0.0/10`, loopback/link-local/IPv6 filtered by `wake::usable`) and prefers a
non-Tailscale answer. Both `HostRow.address` (last that answered) and `HostRow.addresses` (as the
desktop reported in `Status.addresses`) are kept. The Ally does not re-run discovery on its own.

Forgetting: desktop-side forget makes every following Ally request `401` -> `Refused`; Ally-side
removal (`hosts_remove`) deletes the row silently.

---

## 13. The desktop's request pipeline (for someone adding routes)

`crates/companion/src/server.rs`, `serve_with` and `work`:

1. 8 worker threads share one `tiny_http::Server`.
2. `work()` reads headers, decides the body-size cap (section 6.1), whether the request is a long
   one and from loopback, reads the body with a deadline, then:
   * loopback and `/` or `/local/*` -> `Server::local` (section 9);
   * else `Server::handle(&Request{method, path, device, time, nonce, sign, body, from}, now)`.
3. `handle` serves the five open routes, else `trusted` -> `signed`, builds `Reply{status, kind, body,
   sign}`.
4. The response gets `content-type` and `x-carla-sign`; HTML replies get the security headers.
5. Request lines go to `host.log` through a bounded queue and a writer thread so a slow disk never holds
   a worker; unauthenticated noise (anonymous 4xx, `/v1/hello`, unknown routes) is rate-limited into a
   count per 10 s window (`Server::noisy`). Log lines name routes by shape (`route_shape`: `:game`,
   `:folder`...), never by game id, key or file. **If you add a route, add it to `route_shape`'s
   `PLAIN` list (or a prefix), otherwise it logs as `(unknown)` and counts as noise.**
6. `Server::note_seen` records the Ally's address/time (at most every 10 s) and the moment of
   `prepare`.

Handlers share state through `Mutex` fields on `Server`; locks are taken briefly via `locked()`
(poison-tolerant). Do not hold a `Server` lock while doing slow work (`games`, `health`, `scan` use a
separate "currently scanning" mutex for that reason).

---

## 14. How to add a new route safely

Worked example: `GET /v1/foo/{id}` returning `FooInfo`.

1. **Decide whether it needs a route at all.** If it can ride inside an existing response as an
   optional field, do that (rule 2 of section 11). A new field is the cheapest compatible change.
2. **Define the wire type** in `crates/core/src/link/mod.rs` (or the owning module). Derive
   `Debug, Clone, Serialize, Deserialize`; every field an old peer might not send gets
   `#[serde(default)]` or is `Option`; add `skip_serializing_if` if an old reader must not see it.
   Do not use `deny_unknown_fields`.
   **Stop and ask first** if this changes an existing wire type incompatibly: link protocol is a
   shared interface (CLAUDE.md "Explicit user approval").
3. **Client** in `crates/core/src/link/client.rs`: add a `Link` method using `self.ask("GET", path,
   b"", timeout)` or `self.post(path, &body, timeout)`; pick a timeout from `QUICK`/`SLOW`/`PROBE`
   matching how long the handler can take. Return `Result<T>`; use `parse()` for the reply. If an
   old desktop's `404` must not be an error, use `ask_status` and map `404` explicitly (as `retire`
   does) or document the call site.
4. **Server** in `crates/companion/src/server.rs`: add an arm to the `match` in `Server::signed`
   (order matters for prefix routes), implement a private method returning `Answer` (use `json()`,
   `refuse(status, "sentence")`, `body::<T>(req.body)?`). Never call `files.*` or filesystem code
   with a path taken from the request; resolve ids server-side from the desktop's own lists, as
   `cover`, `install` and `saves` do.
5. **Classify the route's cost.** If it can hold a worker for long (sleeps, long I/O, waits on
   another program), add its path to `takes_long`; if it takes a big body from a linked device,
   extend the `most` match in `work()` (otherwise 64 KiB).
6. **Logging:** add the path to `route_shape` (`PLAIN` array or a prefix with a placeholder) so it
   is not logged as unknown.
7. **Authentication:** it must be a signed route unless it is deliberately public (only discovery
   and enrolment are). Never add an open route that changes state or reads user data.
   A new open route is security-sensitive and needs explicit approval.
8. **Compatibility:** an Ally that calls the route on an old desktop gets `404`: decide the
   fallback and write it (section 11 rule 5). A desktop never needs to handle an old Ally calling
   a route it does not know.
9. **Tests** (existing patterns):
   * Unit-test pure logic next to the type (like `sign.rs`, `probe.rs`, `quality.rs`).
   * End-to-end in `crates/companion/src/server.rs` tests: `desk(name, down)` builds a real server
     on a loopback port with a fake Sunshine; `linked(&desk)` links a `Link` through the real
     enrolment; then call your new `Link` method. Add an assertion that a stranger
     (`Link{key: vec![7;32], ..link.clone()}`) gets `Error::Refused`.
   * Client-only behaviour (`401`, unsigned reply, observer) can be tested with the tiny one-shot
     server in `client.rs` tests.
10. **Docs:** add the route to the table in section 6 and any user-visible behaviour to the right
    `docs/*.md`; if the change is durable, add a line to `knowledge/` (see CLAUDE.md "Project
    memory").
11. **Windows/hardware:** nothing in the route table is hardware-verified by a test run alone.
    Claim "in source" and "tested on loopback"; do not claim "works on the Ally" without a device run.

---

## 15. Tests that pin the protocol (where to look)

| Property | Test (file) |
| --- | --- |
| Signature covers method/path/time/nonce/body; reply tied to nonce; replay and window | `sign.rs` tests |
| SPAKE2 same code -> same key; wrong code -> nothing; commit/check | `enrol.rs` tests |
| Full code link then stream; wrong code and 5 tries; strangers/forgotten refused; replay/alter rejected (`409`/`401`) | `server.rs`: `an_ally_links_with_the_code_and_then_streams_a_game`, `a_wrong_code_links_nothing_and_five_tries_use_the_code_up`, `strangers_and_forgotten_devices_are_refused`, `a_captured_request_cannot_be_replayed_or_altered` |
| Approve flow and commit check | the three tests named in section 4.3 |
| Client error mapping and observer | `client.rs` tests (`unreachable`, `refused`, `untrusted`, path scrubbing) |
| Echo/burst arithmetic | `probe.rs` tests; end-to-end on loopback in `discover.rs` `a_companion_answers_a_search` |
| Level selection | `quality.rs` tests |
| Dashboard Host/token checks | `dashboard.rs` tests |
| Backward compatibility of `RemoteGame.description` | `link/mod.rs` `description_tests` |

---

## Open questions / needs investigation

1. **Transport confidentiality.** All traffic on the LAN is cleartext HTTP/UDP. Confirm the project
   position (Tailscale only for remote; LAN accepted) in `knowledge/decisions/` and whether save file
   contents travelling in the clear on the home network is an accepted risk.
2. **Replay protection across restarts.** `Replay` is in memory; confirm whether restart + 300 s
   window is an accepted gap (`sign.rs` `Replay`, `Server.replay`).
3. **Unsigned `503/408/400` replies read as `Untrusted`** by linked Allies (section 5.4). Intended or a
   bug worth a signed variant?
4. **Pre-signature "linked" pre-filter** on `x-carla-device` for body-size caps (section 6.1).
5. **No Ally-side "forget" notification.** The desktop keeps a device until its administrator forgets
   it; confirm this is acceptable (a stolen Ally remains linked).
6. **`AppInfo` on the wire?** Not found in any route; confirm whether it is used only locally.
7. **`API_VERSION` bump policy** and whether enum additions need a compatibility story.
8. **Percent-encoding of ids in paths.** Confirm no id can contain characters that need escaping
   (`emu:` ids, `manual:` keys are hex) and that both sides sign the identical raw path.
9. **Discovery across subnets / Tailscale** is not supported by broadcast; confirm there is no
   secondary mechanism (e.g. MagicDNS) planned.
10. **IPv6.** The server binds IPv4 only and `wake::usable` drops IPv6 addresses; confirm intent.
11. **Moonlight/Sunshine ports** beyond the two probed (`47984`, `47989`) and the firewall rules for
    them are outside this link; see [03](03-desktop-host.md) and `docs/STREAMING.md` for what the
    host sets up.
