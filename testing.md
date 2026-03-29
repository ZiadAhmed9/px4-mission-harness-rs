# PX4 Mission Harness — Testing & Validation Plan

This document is the single authoritative testing plan for the px4-mission-harness-rs project. It covers baseline coverage for the existing codebase, per-phase validation for each roadmap phase, cross-phase regression checks, entry/exit criteria, and the test infrastructure required to execute all tests.

---

## Entry Criteria (before starting any phase)

Before writing or running tests for any phase, the following must all be true:

- [ ] Code compiles with zero warnings: `cargo clippy --workspace -- -D warnings`
- [ ] Code is formatted: `cargo fmt --all -- --check`
- [ ] All pre-existing tests pass: `cargo test --workspace`
- [ ] Feature branch is rebased on main
- [ ] New public APIs have doc comments
- [ ] No new `unwrap()` in `px4-harness-core` — use `expect()` with context or `?` instead

---

## Exit Criteria (before merging any phase)

- [ ] All P0 tests for the phase written and passing
- [ ] All P1 tests for the phase written and passing
- [ ] New code has >= 80% line coverage (measured by `cargo-tarpaulin` or `cargo-llvm-cov`)
- [ ] No HIGH-risk areas left without test coverage
- [ ] All existing `scenarios/*.toml` files parse and validate (verified by unit test calling `ScenarioFile::load()`)
- [ ] JSON report schema has no breaking changes (existing fields preserved)
- [ ] `cargo test` completes in under 30 seconds (excluding SITL-gated tests)
- [ ] SITL integration tests pass (required only for SITL-dependent changes)
- [ ] Manual validation items checked off (required for Phase 5 UI work)

---

## Phase 0 — Baseline Coverage

The existing codebase has several untested modules and weak coverage in others. This phase must be completed before Phase 1 development begins.

### Current Coverage Assessment

| Module | Existing Tests | Coverage |
|--------|---------------|----------|
| `fault/pipeline.rs` | 7 unit tests | Moderate — covers basic stages, misses key edge cases |
| `scenario.rs` | 2 unit tests | Weak — only tests valid parse and one validation case |
| `assertion/engine.rs` | 6 unit tests | Moderate — covers waypoint/altitude/landed basics |
| `mission/controller.rs` | 3 unit tests | Weak — only tests `haversine_distance`, no controller logic |
| `report/json.rs` | 1 unit test | Minimal |
| `report/markdown.rs` | 1 unit test | Minimal |
| `report/junit.rs` | 2 unit tests | Minimal |
| `proxy/udp_proxy.rs` | 0 tests | None |
| `telemetry/processor.rs` | 0 tests | None |
| `telemetry/store.rs` | 0 tests | None |
| `mavlink/connection.rs` | 0 tests | None |
| `error.rs` | 0 tests | None |
| Integration tests (`tests/`) | 0 files | None |

### 0.1 `scenario.rs` Validation Edge Cases (P0)

**Unit Tests:**
- `loss_rate` of exactly 0.0 — expect PASS (valid boundary).
- `loss_rate` of exactly 1.0 — expect PASS (valid boundary).
- `loss_rate` of 1.5 — expect validation error (existing test, keep).
- `loss_rate` of -0.1 — expect validation error (negative value).
- `duplicate_rate` boundary values (0.0, 1.0) — expect PASS; values above 1.0 expect validation error.
- Empty scenario name — document and test whether this is allowed or rejected.
- Waypoint with `acceptance_radius` of 0.0 — expect validation error (or define behavior).
- Waypoint with `acceptance_radius` negative — expect validation error.
- Waypoint `latitude` outside valid range (e.g., 91.0, -91.0) — expect validation error.
- Waypoint `longitude` outside valid range (e.g., 181.0, -181.0) — expect validation error.
- `takeoff_altitude` of exactly 0.0 — expect validation error (boundary of the `> 0.0` check).
- TOML with unknown keys — document whether silent acceptance is intended; if `deny_unknown_fields` is not set, add a test confirming it and a comment explaining why.
- Very large `delay_ms` (e.g., `u64::MAX`) — verify no overflow when converting to `Duration::from_millis`.
- Very large `jitter_ms` — same overflow check.
- `NaN` in float fields — verify `toml::from_str` rejects it or document the behavior; TOML does not support NaN as a literal but paths to it from serde can be surprising.
- `Infinity` in float fields — same as NaN.

### 0.2 `fault/pipeline.rs` Missing Edge Cases (P0)

**Unit Tests:**
- `loss_rate` of exactly 0.0 — verify no packets are ever dropped (the current `>` comparison is correct but untested at boundary).
- `loss_rate` of exactly 1.0 with `burst_loss_length` of 0 — verify the packet is dropped and no burst state is entered (code requires `> 1` to start a burst; test this explicitly).
- `burst_loss_length` of 1 — effectively the same as no burst since `1 - 1 = 0` remaining drops; verify the behavior is intentional.
- Replay buffer at capacity (101+ packets submitted) — verify the oldest entry is evicted and memory does not grow unboundedly.
- Replay stale with an empty buffer (first packet ever processed) — the current packet is the only candidate and it is 0ms old so it will not be found as stale; verify no panic and that the packet is forwarded.
- `duplicate_rate` of 1.0 combined with `replay_stale_ms > 0` and `loss_rate < 1.0` — verify that all three actions (Forward, Duplicate, Replay) are returned in a single `process()` call.
- Zero-length packet (`data: &[]`) — verify no panic in any pipeline stage.

### 0.3 `telemetry/store.rs` — Untested, Concurrent Writes Are a Correctness Risk (P0)

**Unit Tests:**
- `new()` initializes with empty position/status collections and `LandedState::Undefined`.
- `record_position` followed immediately by `latest_position` returns the same sample.
- `record_status` with multiple calls preserves insertion order in the returned slice.
- `update_landed_state` overwrites the previous state; a subsequent read returns the new value.
- Multiple calls to `record_position` from two Tokio tasks concurrently — verify no data is lost and no panic occurs.

**Concurrency Tests:**
- Spawn 10 Tokio tasks each writing 100 position samples concurrently; after all complete, verify the store holds exactly 1000 samples.
- Writer task and reader task (`latest_position`) run concurrently for 1 second — verify no panic and that the reader never observes a partially constructed sample.

### 0.4 `telemetry/processor.rs` — Untested, Message Routing Is a Correctness Risk (P0)

**Unit Tests:**
- `GLOBAL_POSITION_INT` message: verify `lat` and `lon` are divided by 1e7 and `alt` is divided by 1000 to convert to metres and decimal degrees.
- `HEARTBEAT` message: verify the `armed` flag is extracted correctly from the `base_mode` bitfield using the `MAV_MODE_FLAG_SAFETY_ARMED` bit.
- `EXTENDED_SYS_STATE` message: verify all `MavLandedState` variants (`Undefined`, `OnGround`, `InAir`, `Takeoff`, `Landing`) are mapped to the correct internal `LandedState` enum value.
- Message with `system_id != 1` — verify it is forwarded to the output channel but NOT recorded in the telemetry store.
- `GLOBAL_POSITION_INT` message — verify it is both recorded in the store AND forwarded to the output channel.
- Unknown/unsupported message type — verify it is forwarded to the output channel without error and without being stored.

### 0.5 `proxy/udp_proxy.rs` — No Tests (P1)

`UdpProxy` cannot be easily unit-tested without network I/O, but the following approaches are feasible:

**Unit Tests:**
- Port collision: construct a `UdpProxy` where `px4_port` and `proxy_port` are the same value — verify that `start()` returns a clear `AddrInUse` error (not a panic).
- Configuration validation: verify a proxy config with a `proxy_port` of 0 returns an appropriate error.

**Integration Tests (loopback, no SITL required):**
- Bind two UDP sockets on loopback. Start `UdpProxy` between them. Send 10 packets through the proxy with a no-fault profile — verify all 10 arrive at the destination.
- Same setup with `loss_rate = 1.0` — verify zero packets arrive.
- Send a 0-byte UDP datagram — verify no panic.
- Send a 65535-byte UDP datagram (maximum UDP payload) — verify no panic and that the packet is handled (forwarded or dropped per profile).

### 0.6 Property-Based Tests (P1)

Add the `proptest` crate as a dev-dependency. Implement the following property-based tests:

**`haversine_distance` properties:**
- For any two valid (lat, lon) pairs: distance >= 0.0.
- For identical points: distance == 0.0.
- Symmetric: `haversine(a, b) == haversine(b, a)`.
- Triangle inequality: `haversine(a, c) <= haversine(a, b) + haversine(b, c)`.

**`FaultProfile` serde round-trip:**
- Any `FaultProfile` with valid field values serialized to TOML and deserialized back yields a struct equal to the original. This catches serde regressions early, especially when new fields are added in Phase 3.

**`FaultPipeline::process` invariants:**
- For `loss_rate == 0.0` and `duplicate_rate == 0.0`: every call returns at least one `Forward` action (never drops a packet).
- For any valid profile: the number of actions returned per packet is bounded (no unbounded growth).
- For `loss_rate == 1.0`: no `Forward` action is ever returned (every packet is dropped or handled as burst).

---

## Phase 1 — Multi-Mission & Scenario Suites

### 1.1 Suite File Parsing

**Unit Tests:**
- Parse a valid `suite.toml` listing multiple scenario paths — verify all paths are loaded.
- Parse a `suite.toml` with an empty scenario list — expect a validation error.
- Parse a `suite.toml` referencing a non-existent file — expect a clear file-not-found error.
- Parse a `suite.toml` with duplicate scenario paths — decide and test: deduplicate or reject.
- Parse a `suite.toml` where one scenario file exists but contains a parse error — decide and test: fail fast (halt the suite) or continue with remaining scenarios. Document the chosen behavior.
- Parse a `suite.toml` where one file is permission-denied — verify the error message is clear.
- Verify that passing a directory path discovers all `*.toml` files within it.
- Verify that nested subdirectories are handled (either recursed or rejected, per design choice).
- Verify symlinks in a scenario directory are either followed or rejected consistently — document the choice.
- `suite.toml` with the entirely wrong structure (e.g., a valid TOML file but no `scenarios` key) — verify the serde parse error is reported clearly.

**Security / Robustness Tests:**
- `suite.toml` referencing a path that traverses outside the working directory (e.g., `../../etc/passwd`) — verify the loader rejects path traversal or reads it as a missing file rather than silently loading arbitrary system files.
- Deeply nested TOML tables (100 levels deep) — verify `toml::from_str` handles this without stack overflow.
- TOML with a string field of 1 MB — verify parse completes without OOM.

**Edge Cases:**
- A suite containing a single scenario behaves identically to the current `--scenario` flag.
- A suite with 50+ scenarios does not stack overflow or exhaust memory during parsing.

**Performance Tests:**
- Suite of 50 scenario files parses in under 1 second on a standard developer machine.
- Memory use after parsing 50 scenarios is comparable to parsing 1 (no per-scenario allocation leak).

### 1.2 Sequential Suite Execution

**Note on Architecture Prerequisite:** The test "mock mission runner confirms each scenario is invoked in order" requires that `MissionController` is testable via a trait boundary. Currently `MissionController` is a concrete struct owning a `MavlinkConnection` directly; there is no trait to mock against. Before these tests can be written, either a `trait MissionExecutor` must be introduced (with `MissionController` implementing it), or a test-double mechanism must be in place. This is an architectural prerequisite — flag it before beginning Phase 1 implementation.

**Unit Tests (once trait extraction is done):**
- Mock mission runner confirms each scenario is invoked in order.
- If scenario 2 of 3 fails assertions, scenarios 1 and 3 still run to completion.
- Verify that telemetry stores are isolated per scenario (no cross-contamination between runs).
- Memory isolation: after running scenario N, the `TelemetryStore` for scenario N+1 is freshly initialized with no data from scenario N.

**Integration Tests (SITL):**
- Run the suite against `scenarios/` directory with PX4 SITL — all `no_faults.toml` assertions pass.
- Run a suite of `no_faults.toml` + `heavy_loss.toml` — confirm independent results per scenario.

**Concurrency Tests:**
- Verify that shared state (e.g., any global counters or registries) is correctly reset between scenario runs when suite execution is parallelised in a future phase.

### 1.3 Comparative Report

**Unit Tests:**
- Build a comparative report from 3 mock scenario results — verify the matrix contains all scenario names, all assertion results, and all fault summaries.
- Verify Markdown comparative table renders correctly with scenarios of different assertion counts.
- Verify JSON comparative report is valid and parseable.
- Comparative report with 0 shared assertions across scenarios (each scenario has unique assertion names) — verify the matrix renders without panic.

**Manual Validation:**
- Open the Markdown comparative report — visually confirm the table is readable and aligned.
- Pipe the JSON comparative report through `jq` — confirm structure matches expectations.

### 1.4 Exit Code Semantics

**Note:** Exit code 2 for "zero scenarios after validation" is a design decision, not a test decision. Decide whether this is a distinct code (2) or collapses to the general error code (1) before writing the test. Mark the exit-code-2 case below as pending that decision.

**Unit Tests:**
- All scenarios pass — exit code 0.
- One scenario fails — exit code 1.
- All scenarios fail — exit code 1.
- Suite has zero scenarios (after validation) — exit code 2 (or exit code 1 + error message, pending design decision).

**Integration Tests (SITL):**
- Run a passing suite in CI — confirm the pipeline step succeeds.
- Run a failing suite in CI — confirm the pipeline step fails.

---

## Phase 2 — Richer Assertions & Metrics

### 2.1 Timing Assertions

**Unit Tests:**
- Waypoint 2 reached 10s after waypoint 1, timeout is 15s — PASS.
- Waypoint 2 reached 20s after waypoint 1, timeout is 15s — FAIL.
- Waypoint 1 never reached — timing assertion for segment 1-2 reports FAIL with "prerequisite waypoint not reached."
- First waypoint timing is measured from mission start (takeoff complete), not from T=0.

**Edge Cases:**
- Two waypoints reached in the same telemetry sample (very close together) — delta ~0s, should PASS.
- Timeout of 0 seconds — always FAIL (validates boundary).

### 2.2 Geofence Assertion

**Unit Tests:**
- All telemetry samples within bounding box — PASS.
- One sample exceeds max altitude by 0.1m — FAIL; report the offending sample's timestamp and value.
- One sample exceeds max lateral distance from path by 1m — FAIL.
- Empty telemetry store — FAIL with "no telemetry data."
- Position exactly on the boundary (within f64 epsilon) — explicitly test whether the comparison uses `>` or `>=` and assert the documented boundary is respected.
- Waypoints crossing the antimeridian (lon 179 to -179) — verify Haversine handles the wrap-around correctly; note that a naive bounding-box check does not.
- Waypoints at the poles (lat 90 or -90) — verify degenerate distance calculations do not produce NaN or panic.

**Integration Tests (SITL):**
- Run `no_faults.toml` with a tight geofence (50m radius, 30m altitude ceiling) — PASS.
- Run `high_delay.toml` with the same geofence — observe whether overshoot triggers a FAIL.

### 2.3 Velocity/Attitude Assertions

**Unit Tests:**
- Max tilt 15 degrees, all attitude samples under 15 degrees — PASS.
- One attitude sample at 16 degrees — FAIL; report timestamp.
- Max ground speed 10 m/s, all velocity samples under 10 — PASS.
- One velocity sample at 10.5 m/s — FAIL.
- Telemetry store has position samples but zero attitude samples — FAIL with "no attitude data" (not a panic).

**Edge Cases:**
- Tilt must be calculated from both roll and pitch using `acos(cos(roll) * cos(pitch))` (or an equivalent formula), not as `max(abs(roll), abs(pitch))`; write a test that distinguishes these formulas by using a roll-pitch pair where both are 10 degrees (combined tilt ~14.1 degrees > 13 degree limit, but max(10, 10) = 10 would falsely pass).
- Ground speed must be calculated as `sqrt(vx^2 + vy^2)` excluding `vz`; write a test with `vx = 0, vy = 0, vz = 20` (which exceeds a 10 m/s limit if `vz` is wrongly included, but should PASS if correctly excluded).

### 2.4 Quantitative Metrics

**Design Decision Required:** The existing `haversine_distance` function is 2D (ignores altitude). If path length should include vertical distance (e.g., for missions with significant altitude changes), a 3D distance function is needed. Decide before implementing path-length tests whether the metric is 2D or 3D, and document the choice in the function's doc comment.

**Unit Tests:**
- Total flight time computed as last disarm timestamp minus first arm timestamp.
- Path length computed as sum of Haversine distances (2D) between consecutive position samples — note the 2D limitation in the test comment.
- Path length with exactly 1 position sample — should return 0, not an error.
- Path length with 0 position samples — should return 0 or a defined error (document the choice).
- Max path deviation computed against the straight-line segments between waypoints.
- Energy proxy (sum of |velocity| * dt) computed correctly for a known velocity profile.
- Energy proxy with `dt = 0` between two consecutive samples (same timestamp) — verify no division by zero.

**Manual Validation:**
- Run a mission, check the metrics section in the Markdown report — values should be physically reasonable (e.g., path length roughly equal to straight-line distance for no-fault runs).

---

## Phase 3 — Dynamic Fault Injection (Time-Based Profiles)

### 3.1 Phased Fault Profile Parsing

**Note on TOML Field Name:** The current `ScenarioFile` struct uses `faults` as the field name (not `fault_profile`). Any tests that reference the TOML key for the static fault block must use `[faults]`, not `[fault_profile]`. Verify the actual field name before writing backward-compatibility tests, or they will fail for the wrong reason.

**Unit Tests:**
- Parse a TOML with `[[fault_phases]]` array — verify each phase has the correct trigger, duration, and fault parameters.
- Parse a TOML with zero `[[fault_phases]]` entries — falls back to the existing static `[faults]` block unchanged (this is a P0 regression test).
- Parse a TOML with both `[faults]` and `[[fault_phases]]` — decide and test: return an error, or let phases override static. Document the chosen behavior.
- Overlapping time windows (phase 1: 0–30s, phase 2: 20–50s) — validate or reject at parse time; document and test the chosen behavior.
- Fault phase with `duration_secs = 0` — decide and test: no-op or validation error.
- Existing `scenarios/*.toml` files that have no `[[fault_phases]]` key must parse and run identically to before this change (TOML backward-compatibility regression).

**Security / Robustness Tests:**
- TOML with a `[[fault_phases]]` array containing 10 million entries — verify parse does not OOM.
- Fault phase with `duration_secs = u64::MAX` — verify no overflow when computing phase end time.

### 3.2 Time-Triggered Faults

**Design Decision Required:** Document whether fault phases use wall-clock time or mission-elapsed time (from first arm). This decision affects how tests are written and how reproducibility is guaranteed. Flag this before implementing.

**Unit Tests:**
- Pipeline at T=10s with a fault phase starting at T=30s — no faults applied, packet forwarded cleanly.
- Pipeline at T=35s with a fault phase starting at T=30s, duration 15s — faults are active.
- Pipeline at T=50s with the same phase — faults have expired, packet forwarded cleanly.
- Three overlapping time phases with conflicting fault params — the composition semantics (most-severe wins, additive, or error) must be decided before this test is written; flag as pending design decision.
- Phase transition during an in-flight delayed packet — verify whether the delay is committed at the time it is applied (old phase) or re-evaluated at delivery time (new phase); document and test.

**Integration Tests (SITL):**
- Scenario: no faults for first 20s, then 50% loss for 10s, then no faults. Verify the mission completes but telemetry shows a gap during the fault window.
- Scenario: 1000ms delay for 5s at T=15s. Verify the waypoint is still reached but the timing assertion captures the delay.

**Performance Tests:**
- `FaultPipeline` with 10 phase transitions per second sustains 100 packets/sec (simulated with a tight loop) with under 1ms per-packet overhead on average.
- Rapid phase switching (a new phase every 100ms for 60s) — verify no memory growth in the phase table.

### 3.3 Event-Triggered Faults

**Unit Tests:**
- Fault triggers on "waypoint 2 reached" — verify the pipeline activates only after the telemetry store records waypoint 2 within acceptance radius.
- Event never fires (waypoint never reached) — fault phase never activates.
- Event fires at mission end (landing) — fault phase activates but has no observable effect (no further packets to degrade).
- Event-triggered fault on a waypoint that is reached multiple times (loiter pattern) — decide and test: fires once on first reach, or fires on every reach. Document the behavioral contract.

**Integration Tests (SITL):**
- Trigger 30% loss when the vehicle reaches waypoint 1 — verify waypoint 2 timing is affected but waypoint 1 timing is clean.

### 3.4 Transition Between Phases

**Unit Tests:**
- Phase 1 (0% loss) ends at T=30s, phase 2 (40% loss) starts at T=30s — no gap, no overlap; verify transition is instantaneous.
- Phase 1 ends at T=30s, phase 2 starts at T=35s — 5s gap with no faults applied during the gap.
- Rapid phase switching (a new phase every 1 second for 30 phases) — pipeline handles without lag or memory growth.

---

## Phase 4 — Multi-Vehicle Support

### 4.1 Per-Vehicle Configuration

**Unit Tests:**
- Parse a TOML with `[[vehicles]]` array, each with system ID, ports, and fault profile — verify correct deserialization.
- Duplicate system IDs — expect validation error.
- Duplicate port numbers — expect validation error.
- Single vehicle config degrades gracefully to current single-vehicle behavior.

### 4.2 Proxy Multiplexing

**Unit Tests:**
- Two mock vehicles send packets with different system IDs — verify packets are routed through independent fault pipelines.
- Vehicle 1 has 0% loss, vehicle 2 has 100% loss — vehicle 1 packets forwarded, vehicle 2 packets dropped.
- Unknown system ID packet arrives — decide and test: drop, forward without faults, or log a warning.
- System ID 0 (MAVLink broadcast) — decide and test how the proxy handles it; this is a protocol compliance question.
- System ID 255 conflict (if the proxy sends its own heartbeats with sys_id=255 and a vehicle config also uses 255) — verify no routing confusion or duplicate fault application.

**Integration Tests (SITL, multi-instance):**
- Start two PX4 SITL instances. Run both through the proxy with different fault profiles. Verify independent telemetry streams and assertion results.

### 4.3 Inter-Vehicle Assertions

**Unit Tests:**
- Two vehicles always >50m apart, minimum separation assertion is 30m — PASS.
- Two vehicles pass within 10m at T=25s, minimum separation is 30m — FAIL; report timestamp and distance.
- One vehicle has no position data — FAIL with "insufficient telemetry."
- Three vehicles — all pairwise separations are checked, not just the first pair.

**Edge Cases:**
- Telemetry sample rates differ between vehicles — interpolation or nearest-sample matching is needed; document and test the chosen strategy.

### 4.4 Multi-Vehicle Reports

**Unit Tests:**
- Report contains per-vehicle sections with individual assertion results.
- Comparative view shows all vehicles side-by-side.
- JUnit XML emits one `<testsuite>` per vehicle.

---

## Phase 5 — Live Dashboard & Observability

### 5.1 TUI Dashboard (ratatui)

**Unit Tests:**
- Render function produces correct terminal output for a known state (mock telemetry, mock fault stats).
- Dashboard handles zero telemetry samples without panic.
- Dashboard handles rapid updates (100 samples/sec) without blocking the proxy or mission controller.
- TTY detection logic: when stdout is not a TTY (e.g., piped to a file), the dashboard is disabled. Write a unit test for the detection predicate rather than relying solely on manual validation.

**Manual Validation:**
- Run a mission with `--dashboard` flag — verify live position updates, fault counters incrementing, and assertion status changing in real time.
- Resize terminal during a run — dashboard reflows without crash.
- Run without a TTY (piped output) — dashboard is disabled gracefully, falls back to text output.

**Performance Tests:**
- Dashboard render at 60fps with 1000 telemetry samples in the store does not block proxy tasks (measure proxy task latency before and after enabling the dashboard).

### 5.2 Prometheus Metrics Export

**Note on SITL Dependency:** The unit tests below do not require SITL — they inject mock data directly. Only the curl/Grafana integration tests require a running harness. Label accordingly.

**Unit Tests (no SITL required):**
- Metrics endpoint returns valid Prometheus text format when given mock data.
- Counters `packets_forwarded`, `packets_dropped`, `packets_duplicated`, `packets_replayed` increment correctly when the corresponding fault pipeline actions are recorded.
- Gauges `vehicle_latitude`, `vehicle_longitude`, `vehicle_altitude` update to the latest telemetry sample value.
- Histogram `packet_delay_seconds` records the correct bucket distributions for known delay values.
- Prometheus metric names follow naming conventions: no spaces, only alphanumeric characters and underscores, no leading digits.
- Metrics endpoint handles two concurrent scrapers simultaneously without data races.

**Integration Tests (SITL required):**
- Start harness with `--metrics-port 9090`. Curl `localhost:9090/metrics` during a run — verify valid Prometheus output.
- Point Grafana at the endpoint — verify dashboard panels populate.

### 5.3 Structured Event Log

**Unit Tests:**
- Event log records arm, takeoff, waypoint reached, land, and disarm events with correct timestamps.
- Event log records fault application events (packet dropped, delayed, duplicated).
- JSON export of event log is valid and parseable.
- Event log under high fault injection rate (simulated at 10,000 events/sec) — verify memory stays within the documented buffer bound (no unbounded growth).

**Manual Validation:**
- Run a full mission, export the event log — verify the timeline makes sense chronologically.
- Pipe the JSON event log through `jq` — verify structure.

---

## Phase 6 — Scenario Generation & Fuzzing

### 6.1 Parameter Sweep

**Unit Tests:**
- Sweep `loss_rate` from 0.0 to 0.5 in 0.1 steps — verify 6 scenarios are generated with correct parameters.
- Sweep two parameters simultaneously (`loss_rate` x `delay_ms`) — verify the Cartesian product (e.g., 6 x 5 = 30 scenarios).
- Sweep with step size that does not evenly divide the range — verify the last value is clamped to max (not extrapolated past it).
- Step size of 0 — verify a validation error is returned (not an infinite loop or division by zero).
- `min > max` in sweep range — verify validation rejects or swaps the values; document the behavior.
- All generated scenarios pass `ScenarioFile::validate()`.

**Integration Tests (SITL):**
- Sweep `loss_rate` 0% to 30% in 10% steps — run all 4 generated scenarios. Verify a degradation curve appears in the comparative report.

**Performance Tests:**
- Parameter sweep of 100 scenarios generates and validates all scenarios in under 10x the time of a single scenario parse/validate.

### 6.2 Binary Search for Failure Threshold

**Unit Tests:**
- Mock mission runner: `loss_rate < 0.25` passes, `>= 0.25` fails. Binary search converges to 0.25 within 0.01 tolerance in 10 or fewer iterations.
- All runs pass (threshold above max) — reports "no failure found in range."
- All runs fail (threshold below min) — reports "fails even at minimum."
- Non-monotonic results (flaky mission) — search reports "inconclusive, results are non-monotonic" or uses majority voting. Document the chosen strategy.

**Integration Tests (SITL):**
- Search for `loss_rate` failure threshold on a simple mission — verify the reported threshold is reproducible (run the threshold value 3 times; the majority should fail).

### 6.3 Random Fault Fuzzing

**Unit Tests:**
- Generate 100 random scenarios within bounds — all pass `ScenarioFile::validate()`.
- Random seed is recorded in each generated scenario — re-running with the same seed produces identical fault parameters.
- `--max-iterations 0` — verify a validation error is returned (not zero iterations silently completing as "pass").
- Random seed reproducibility across platforms — verify that `rand` with a fixed seed produces the same sequence on Linux and macOS (document if this is not guaranteed).
- Results are aggregated: report shows which parameter combinations caused failures.

**Integration Tests (SITL):**
- Run 10 random fuzzing iterations — verify all complete without crash, and the report shows pass/fail per iteration.

**Manual Validation:**
- Review the fuzzing summary report — verify it highlights the most failure-prone parameter regions.

---

## Cross-Phase Regression Tests

After each phase, all of the following must still pass:

| Check | Command | Expected |
|-------|---------|----------|
| All unit tests pass | `cargo test` | 0 failures |
| Clippy clean | `cargo clippy --workspace -- -D warnings` | 0 warnings |
| Format clean | `cargo fmt --all -- --check` | No changes needed |
| Existing scenarios parse | Unit test calling `ScenarioFile::load()` on each file in `scenarios/` | All load without error |
| SITL integration (if available) | `cargo test -p px4-harness-core --features sitl` | All pass |
| CLI `--help` reflects new flags | `cargo run -p px4-harness -- --help` | New options listed |
| JSON report schema unchanged | Diff against previous JSON output | No breaking changes to existing fields |
| Markdown report readable | Open generated report | No formatting regressions |
| Test suite time | `cargo test` (excluding SITL-gated tests) | Completes in under 30 seconds |
| No new `unwrap()` in library | `grep -rn "\.unwrap()" crates/px4-harness-core/src/` | Zero new occurrences |
| Dependency audit | `cargo deny check` | No license violations, no known vulnerabilities |

**Note on the scenario parse check:** The previous check `cargo run -p px4-harness -- -s scenarios/no_faults.toml --help` does NOT exercise scenario parsing because `--help` causes early exit before the scenario file is loaded. Replace it with a dedicated unit test (or integration test) that calls `ScenarioFile::load()` on every file under `scenarios/` and asserts no errors are returned.

---

## Test Infrastructure Needed

### Phases 0–3 (minimal external deps)

- Existing `#[cfg(test)]` module pattern is sufficient for unit tests.
- All async tests must use `#[tokio::test]` (not `#[test]`). This applies to any test that calls `.await`, uses `tokio::spawn`, or interacts with Tokio channels or timers.
- SITL integration tests continue to use the `sitl` feature gate.
- `proptest` dev-dependency for property-based tests (Phase 0 and beyond). Add to `px4-harness-core/Cargo.toml` under `[dev-dependencies]`.
- `tempfile` dev-dependency for file-system tests in the suite runner (Phase 1). Required for creating temp directories with TOML files in unit tests.
- `cargo-tarpaulin` (or `cargo-llvm-cov`) in CI to enforce the >= 80% line coverage exit criterion.

### Architectural Prerequisites (Phase 1)

- **`trait MissionExecutor`** (or equivalent): `MissionController` must be mockable before Phase 1 sequential-execution tests can be written. Without a trait, the only option is live SITL. Introduce the trait before Phase 1 development begins.
- **Mock `MavlinkConnection`**: Similarly, `MavlinkConnection` has no trait boundary, making any test that touches the mission controller require either a real connection or `mockall`-style injection. Decide on the approach (trait + `mockall`, or conditional compilation with a test double) and document it here once chosen.
- **Shared test fixtures**: The `sample_report()` helper function is currently duplicated across `report/json.rs`, `report/markdown.rs`, and `report/junit.rs` test modules. Extract it into a shared `#[cfg(test)]` utility module or a `tests/common/` helper before Phase 1 adds more report variants.

### Phase 4 (multi-vehicle)

- CI or local setup must be able to spawn 2+ PX4 SITL instances on different ports.
- Consider a `multi-sitl` feature flag for these tests to keep `cargo test` fast.

### Phase 5 (dashboard/metrics)

- TUI snapshot tests: `insta` crate for terminal output comparison across code changes.
- Prometheus HTTP tests: `reqwest` (or a similar HTTP client) as a dev-dependency for integration tests that curl the metrics endpoint.

### Phase 6 (fuzzing)

- Fuzzing tests are inherently long-running. Gate behind a `fuzz` feature flag.
- `--max-iterations` cap must be enforced in CI to keep run times bounded.

---

## Risk-Based Coverage Map

This table provides a quick reference for prioritizing test work. Whenever a module is modified, ensure its coverage tier is maintained.

| Component | Risk Rating | Risk Category | Current Coverage | Key Gaps |
|-----------|-------------|---------------|-----------------|----------|
| `scenario.rs` parsing + validation | HIGH | Data Integrity | Weak (2 tests) | Boundary values, NaN, unknown fields, negative values |
| `fault/pipeline.rs` | HIGH | Correctness | Moderate (7 tests) | Boundary rates, replay buffer capacity, action composition |
| `telemetry/processor.rs` | HIGH | Correctness | None | Unit conversion, bitfield parsing, system ID filtering |
| `telemetry/store.rs` | MEDIUM | Concurrency | None | Mutex poisoning, ordering, concurrent writes |
| `assertion/engine.rs` | HIGH | Correctness | Moderate (6 tests) | Empty telemetry, float boundaries, edge timestamps |
| `proxy/udp_proxy.rs` | HIGH | Reliability | None | Needs trait extraction or loopback integration test |
| `mavlink/connection.rs` | MEDIUM | Reliability | None | Only testable with SITL or mock |
| `report/*.rs` | LOW | Data Integrity | Minimal (4 tests) | Adequate for current scope; extract shared fixture |
| `mission/controller.rs` | HIGH | Correctness | Weak (3 tests, haversine only) | Needs trait extraction for controller logic testing |
| Phase 1: Suite runner | HIGH | Correctness | N/A (not built) | Mock infrastructure prerequisite; memory isolation |
| Phase 2: New assertions | HIGH | Correctness | N/A (not built) | Float edge cases, missing data, antimeridian handling |
| Phase 3: Dynamic faults | HIGH | Correctness + Timing | N/A (not built) | Phase composition design decision; backward compat |
| Phase 4: Multi-vehicle | MEDIUM | Correctness | N/A (not built) | Routing, system ID edge cases, interpolation |
| Phase 5: Dashboard | LOW | UX | N/A (not built) | Mostly manual validation; TTY detection unit test |
| Phase 6: Fuzzing | MEDIUM | Correctness | N/A (not built) | Input validation, seed reproducibility |
