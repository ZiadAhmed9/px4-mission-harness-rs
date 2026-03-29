# Testing Strategy Review: testing.md vs plan.md

## Executive Summary

The `testing.md` document is a solid starting point that covers the happy-path and basic error cases for each phase. However, after reviewing it against the actual codebase, the `plan.md` requirements, and standard risk-based testing practices, there are significant gaps in five areas:

1. **No testing strategy for the existing codebase** -- testing.md only covers future phases, ignoring gaps in what is already built
2. **Missing entire test levels** -- no performance tests, no security tests, no property-based/fuzz tests for parsers
3. **Weak entry/exit criteria** -- the regression table is necessary but not sufficient; there are no measurable coverage thresholds or risk-gated exit criteria
4. **Concurrency and async risks are undertested** -- the proxy, telemetry processor, and mission controller all use shared mutable state under `Mutex` and async channels, but no tests target concurrency hazards
5. **Missing edge cases unique to this domain** -- floating-point GPS comparison, MAVLink protocol edge cases, UDP transport behavior

---

## 1. Gap: No Strategy for the Existing Codebase

`testing.md` starts at Phase 1 and assumes the current code is already well-tested. It is not. Here is what exists today and what is missing:

### Current Test Coverage (from code review)

| Module | Existing Tests | Coverage Assessment |
|--------|---------------|---------------------|
| `fault/pipeline.rs` | 7 unit tests | **Moderate** -- covers basic stages but misses key edge cases (see below) |
| `scenario.rs` | 2 unit tests | **Weak** -- only tests valid parse and one validation case |
| `assertion/engine.rs` | 6 unit tests | **Moderate** -- covers waypoint/altitude/landed basics |
| `mission/controller.rs` | 3 unit tests | **Weak** -- only tests `haversine_distance`, no controller logic |
| `report/json.rs` | 1 unit test | **Minimal** |
| `report/markdown.rs` | 1 unit test | **Minimal** |
| `report/junit.rs` | 2 unit tests | **Minimal** |
| `proxy/udp_proxy.rs` | 0 tests | **None** |
| `telemetry/processor.rs` | 0 tests | **None** |
| `telemetry/store.rs` | 0 tests | **None** |
| `mavlink/connection.rs` | 0 tests | **None** |
| `error.rs` | 0 tests | **None** |
| Integration tests (`tests/`) | 0 files | **None** |

### Recommendation: Add a "Phase 0 -- Baseline Coverage" Section

Before building new features, the existing code has untested HIGH-risk areas. Add a section to `testing.md` covering:

**P0 -- Must add before Phase 1 work begins:**

- **`scenario.rs` validation edge cases:**
  - `loss_rate` exactly at boundaries: 0.0, 1.0 (currently only tests 1.5)
  - `loss_rate` of -0.1 (negative)
  - `duplicate_rate` boundary values (same pattern)
  - Empty scenario name (is it allowed?)
  - Waypoint with `acceptance_radius` of 0.0 or negative
  - Waypoint latitude/longitude outside valid ranges (lat > 90, lon > 180)
  - `takeoff_altitude` of exactly 0.0 (boundary of the > 0.0 check)
  - TOML with unknown keys (serde `deny_unknown_fields` is NOT enabled -- is silent acceptance intended?)
  - Very large `delay_ms` or `jitter_ms` values (e.g., `u64::MAX`) -- could overflow in `Duration::from_millis`
  - `NaN` and `Infinity` in float fields -- serde/TOML behavior here is non-obvious

- **`fault/pipeline.rs` missing edge cases:**
  - `loss_rate` of exactly 0.0 -- verify no drops (the current `>` check is correct but untested at boundary)
  - `loss_rate` of exactly 1.0 with `burst_loss_length` of 0 -- does it start a burst or not? (Code says `> 1`, so no burst. Test this.)
  - `burst_loss_length` of 1 -- effectively same as no burst since `1 - 1 = 0` remaining. Verify.
  - Replay buffer at capacity (101+ packets) -- verify oldest packet is evicted
  - Replay stale with empty buffer (first packet ever) -- the replay buffer has the current packet which is 0ms old, so it won't be found. Verify.
  - `duplicate_rate` of 1.0 combined with `replay_stale_ms > 0` -- verify 3 actions returned (original + duplicate + replay)
  - Zero-length packet (`data: &[]`) -- verify no panic

- **`telemetry/store.rs` -- untested, concurrent writes are a correctness risk:**
  - `record_position` followed by `latest_position` returns the correct sample
  - `record_status` ordering is preserved
  - `update_landed_state` overwrites previous state
  - `new()` initializes with empty collections and `LandedState::Undefined`

- **`telemetry/processor.rs` -- untested, message routing is a correctness risk:**
  - `GLOBAL_POSITION_INT` message is correctly converted (lat/lon divided by 1e7, alt by 1000)
  - `HEARTBEAT` message correctly extracts `armed` flag from `base_mode` bitfield
  - `EXTENDED_SYS_STATE` maps all `MavLandedState` variants correctly
  - Messages from `system_id != 1` are ignored (not stored in telemetry)
  - Messages are forwarded to the output channel even after being processed
  - Unknown message types are forwarded without being stored

---

## 2. Gap: Missing Test Levels

### 2.1 No Performance Tests Anywhere

`testing.md` does not mention performance testing for any phase. This matters because:

- **Phase 1 (Suite Runner):** Running 50+ scenarios sequentially -- what is the memory footprint? Does telemetry from scenario N leak into scenario N+1?
- **Phase 3 (Dynamic Faults):** Rapid phase switching (every 1s) is mentioned as a unit test but there is no assertion about latency overhead or memory growth.
- **Phase 5 (Dashboard):** "100 samples/sec without blocking" is mentioned but there is no performance test to verify it.
- **Existing proxy:** What is the throughput of the fault pipeline under sustained 100Hz MAVLink traffic? The `FaultPipeline::process()` method allocates a `Vec<FaultAction>` per packet. Under high rates, this creates GC pressure (Rust doesn't have GC, but allocator pressure is real).

**Recommendation:** Add a performance test section per phase. At minimum:

```
### Performance Tests (per phase)
- Phase 1: Suite of 50 scenarios parses in < 1 second. Memory does not grow across runs.
- Phase 3: FaultPipeline with 10 phase transitions per second sustains 100 packets/sec with < 1ms per-packet overhead.
- Phase 5: Dashboard render at 60fps with 1000 telemetry samples does not block proxy tasks.
- Phase 6: Parameter sweep of 100 scenarios completes within 10x single-scenario time.
```

### 2.2 No Security/Robustness Tests

The harness processes:
- Untrusted TOML files (scenarios could come from a shared repo)
- Raw UDP packets (MAVLink from the network)
- User-supplied file paths (CLI arguments)

**Missing tests:**

- **Malformed TOML:** Deeply nested tables (100 levels), arrays with 10 million elements, strings of 1GB. Does `toml::from_str` handle these gracefully or OOM?
- **Malformed MAVLink packets:** The telemetry processor receives messages already parsed by the `mavlink` crate. But the UDP proxy forwards raw bytes. What happens if a 0-byte UDP packet arrives? A 65535-byte packet? The proxy currently handles this (the buffer is 65535), but there are no tests.
- **Port collision:** What if `px4_port` and `proxy_port` are the same? The `UdpProxy::start` will get an `AddrInUse` error, but is the error message clear?
- **Path traversal in suite runner (Phase 1):** If `suite.toml` references `../../etc/passwd`, does the loader reject it or read arbitrary files?

**Recommendation:** Add a `### Security / Robustness Tests` section, at least for Phases 1 and 3 where untrusted input parsing is expanded.

### 2.3 No Property-Based Tests

Several components are perfect candidates for property-based testing with the `proptest` crate:

- **`haversine_distance`**: For any two points, distance >= 0. For identical points, distance == 0. Triangle inequality holds. Symmetric: d(a,b) == d(b,a).
- **`FaultPipeline::process`**: For `loss_rate == 0.0`, every packet is forwarded (never dropped). For any profile, the number of actions is bounded.
- **Scenario serde round-trip**: Any `FaultProfile` serialized to TOML and deserialized back yields the same struct. (This would catch serde issues early, especially when new fields are added in Phase 3.)
- **Report serde round-trip**: Any `Report` serialized to JSON and deserialized back yields the same struct.

**Recommendation:** Add a note about property-based testing for at least `haversine_distance` and `FaultProfile` round-trips. These are low-effort, high-value.

---

## 3. Gap: Phase-Specific Issues

### Phase 1 -- Multi-Mission & Scenario Suites

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| Symlinks in scenario directory -- follow or reject? | File system edge case | P1 |
| Suite with scenario files that have parse errors -- does the suite fail fast or continue? | Behavior contract unclear | P0 |
| Suite with permission-denied on one scenario file -- same question | Robustness | P1 |
| Concurrent suite execution (parallel SITL) -- how is port allocation handled? | Port conflict = silent data corruption or bind failure | P0 |
| Comparative report with 0 shared assertions across scenarios -- does the matrix render? | Edge case for matrix layout | P1 |
| Suite TOML schema validation -- what if `suite.toml` has the wrong structure entirely? | Serde parse error quality | P1 |
| Memory isolation: after running scenario N, the `TelemetryStore` for scenario N+1 is fresh | Data contamination across runs | P0 |

**Assessment of existing tests:** The tests for 1.1-1.4 are reasonable for happy path but lack failure-mode coverage. The "mock mission runner" in 1.2 is described but there is no testable abstraction in the current code -- `MissionController` directly owns a `MavlinkConnection`. To mock the mission runner, you will need to introduce a trait (e.g., `trait MissionRunner`) first. This is an architectural prerequisite that `testing.md` should call out.

### Phase 2 -- Richer Assertions & Metrics

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| Floating-point edge cases in geofence: position exactly on boundary (within f64 epsilon) | Floating-point comparison correctness | P0 |
| Geofence with waypoints crossing the antimeridian (lon 179 to -179) | Haversine handles this but bounding box does not | P1 |
| Geofence with waypoints at the poles (lat 90/-90) | Degenerate case for distance calculations | P2 |
| Path length with only 1 position sample -- should be 0, not error | Boundary | P1 |
| Path length with 0 samples -- should report error or 0? | Boundary | P1 |
| Energy proxy with `dt = 0` between consecutive samples (same timestamp) -- division by zero? | Depends on implementation | P0 |
| Velocity/attitude assertions when telemetry store has positions but no attitude samples | Missing data handling | P1 |
| Tilt calculation: verify it uses `acos(cos(roll) * cos(pitch))` or equivalent, not just `max(roll, pitch)` | Correctness | P0 |
| Ground speed calculation: verify vz is excluded (documented in testing.md but needs explicit test with vz >> 0) | Correctness | P0 |

**Assessment:** The timing assertion tests are well-designed. The geofence and velocity tests need float comparison tolerance specified -- "exceeds by 0.1m" depends on whether comparison uses `>` or `>=`, and this should be explicitly tested at boundary.

### Phase 3 -- Dynamic Fault Injection

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| Fault phase with `duration_secs = 0` -- is it a no-op or an error? | Ambiguous behavior | P0 |
| Event-triggered fault on a waypoint that is reached multiple times (loiter pattern) -- fires once or every time? | Behavioral contract | P0 |
| Time-triggered fault with mission clock vs wall clock -- which is used? | Correctness | P0 |
| Three overlapping phases with different fault params -- composition semantics undefined in plan.md | Design decision needed before testing | P0 |
| Phase transition during an in-flight delayed packet -- does the delay change or is it committed? | Subtle timing bug | P1 |
| TOML backward compatibility: scenario with no `[[fault_phases]]` must still work exactly as before | Regression | P0 |

**Assessment:** Section 3.4 (transition tests) is good. The event-triggered fault tests in 3.3 are adequate. The main gap is that composition semantics (overlapping phases) are flagged as a design decision ("or they compose") but never resolved -- the test plan cannot be complete until this is decided.

### Phase 4 -- Multi-Vehicle

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| System ID 0 (broadcast) -- how does the proxy handle it? | Protocol compliance | P1 |
| System ID conflict between proxy's own heartbeats (sys_id=255) and a vehicle config using sys_id=255 | Edge case | P1 |
| More than 2 vehicles -- does the proxy scale? (Hash map vs. vec for routing) | Performance | P2 |
| Vehicle that never sends any packets (powered off) -- does it block the suite or timeout? | Robustness | P1 |
| Inter-vehicle assertion with 3+ vehicles and different telemetry rates -- interpolation correctness | Floating point, timing | P0 |

### Phase 5 -- Dashboard & Observability

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| Dashboard disabled when stdout is not a TTY (e.g., piped to file) | Listed in manual validation but needs a unit test for the detection logic | P1 |
| Prometheus metric names follow naming conventions (no spaces, valid characters) | Protocol compliance | P1 |
| Event log under high fault injection rate -- does it OOM? (bounded buffer mentioned but not tested) | Resource exhaustion | P0 |
| Metrics endpoint concurrency -- two Grafana instances scraping simultaneously | Thread safety | P1 |

### Phase 6 -- Scenario Generation & Fuzzing

**Missing tests:**

| Missing Test | Risk | Priority |
|---|---|---|
| Parameter sweep with `step = 0` -- infinite loop? | Division by zero or infinite generation | P0 |
| Parameter sweep where `min > max` -- validate or swap? | Input validation | P1 |
| Binary search with flaky results (non-monotonic) -- documented but testing strategy is vague | Algorithmic correctness | P1 |
| Random seed reproducibility across platforms (is `rand` deterministic across OS?) | Portability | P1 |
| Fuzzing iteration limit enforcement -- what if `--max-iterations` is 0? | Boundary | P1 |

---

## 4. Gap: Regression / Entry / Exit Criteria

### Current Regression Table Assessment

The cross-phase regression table is a good start but has gaps:

**What is there and works well:**
- `cargo test`, `cargo clippy`, `cargo fmt` -- standard Rust CI gates
- JSON schema backward compatibility check
- SITL integration gate

**What is missing:**

| Missing Check | Why It Matters |
|---|---|
| **No code coverage threshold** | You cannot tell if new code is tested. Recommend: new code in `px4-harness-core` must have >= 80% line coverage (use `cargo-tarpaulin` or `cargo-llvm-cov`). |
| **No `unwrap()` audit** | The codebase currently has `unwrap()` calls in library code (e.g., `positions.lock().unwrap()` in `telemetry/store.rs`, `pipeline.lock().unwrap()` in `udp_proxy.rs`). Mutex poisoning will cause panics. Testing.md should require: "no new `unwrap()` in `px4-harness-core` -- use `expect()` with context at minimum, prefer `?` where possible." |
| **No `cargo deny` or dependency audit** | New phases add dependencies (`ratatui`, `prometheus`, `reqwest`). License and vulnerability scanning should be a gate. |
| **No backward-compatibility test for TOML schema** | The JSON report schema check is good, but scenario TOML files are the primary user interface. All existing `scenarios/*.toml` files must parse and validate with each change. |
| **Existing scenario smoke test is wrong** | The check `cargo run -p px4-harness -- -s scenarios/no_faults.toml --help` does not actually parse the scenario -- `--help` causes early exit before scenario loading. This should be `cargo run -p px4-harness -- -s scenarios/no_faults.toml --dry-run` (which does not exist yet) or a dedicated unit test. |
| **No test execution time budget** | `cargo test` should complete in under 30 seconds (excluding SITL). If Phase 3/6 adds slow tests, they need a feature gate. |

### Recommended Entry Criteria (add to testing.md)

```
### Entry Criteria (before starting testing for any phase)
- [ ] Code compiles with zero warnings: `cargo clippy --workspace -- -D warnings`
- [ ] Code is formatted: `cargo fmt --all -- --check`
- [ ] All pre-existing tests pass: `cargo test --workspace`
- [ ] Feature branch is rebased on main
- [ ] New public APIs have doc comments
- [ ] No new `unwrap()` in px4-harness-core (use `expect()` or `?`)
```

### Recommended Exit Criteria (add to testing.md)

```
### Exit Criteria (before merging any phase)
- [ ] All P0 tests for the phase written and passing
- [ ] All P1 tests for the phase written and passing
- [ ] New code has >= 80% line coverage (measured by cargo-tarpaulin)
- [ ] No HIGH-risk areas without test coverage
- [ ] All existing scenarios/ TOML files parse and validate
- [ ] JSON report schema has no breaking changes (existing fields preserved)
- [ ] cargo test completes in < 30s (excluding SITL-gated tests)
- [ ] SITL integration tests pass (if SITL-dependent changes)
- [ ] Manual validation items checked off (for phases with UI: 5)
```

---

## 5. Gap: Concurrency and Async Testing

This is the most significant testing gap in `testing.md`. The codebase uses:
- `Arc<Mutex<T>>` for shared telemetry store (read by assertions, written by telemetry processor)
- `Arc<Mutex<FaultPipeline>>` for proxy (read/written by async tasks)
- `Arc<Mutex<Option<SocketAddr>>>` for address learning in proxy
- `mpsc::UnboundedReceiver` for message passing between tasks
- `AtomicI32` for target position (written by controller, read by setpoint task)
- `tokio::select!` for proxy task lifecycle

**None of these concurrency patterns are tested.** Specific risks:

| Risk | Impact | Recommended Test |
|---|---|---|
| Mutex poisoning in `TelemetryStore` -- if any thread panics while holding the lock, all subsequent lock attempts panic | Complete harness crash | Unit test: poison a mutex, verify the store method returns an error instead of panicking (requires changing `unwrap()` to proper error handling) |
| Unbounded channel backpressure -- `mpsc::unbounded_channel` can grow without limit if the consumer is slow | OOM on long missions | Integration test: send 100,000 messages through the channel, verify memory stays bounded |
| `tokio::select!` in proxy -- if one direction task panics, the other is cancelled | Silent loss of half the proxy | Integration test: simulate a panic in one proxy direction, verify the proxy shuts down cleanly (not silently) |
| `AtomicI32` ordering -- `Ordering::Relaxed` is used for target position. On x86 this works, but on ARM it can produce stale reads | Wrong waypoint target on ARM | This is acceptable for the current use case (non-safety-critical test harness) but should be documented. Add a comment and a note in testing.md. |
| Race between `start_recv_task` and `run_mission` -- if no messages arrive before `wait_for_ready` timeout | Timeout failure that looks like PX4 not being ready | Integration test (SITL): verify that the harness handles slow PX4 startup gracefully |

**Recommendation:** Add a "### Concurrency Tests" subsection to each phase that introduces shared state. For the existing code, add to Phase 0.

---

## 6. Gap: Testing Infrastructure

`testing.md` has a good "Test Infrastructure Needed" section at the bottom. Additions needed:

| Item | Phase | Why |
|---|---|---|
| `proptest` dependency for property-based testing | Phase 0/1 | Catch edge cases in haversine, serde, fault pipeline |
| `cargo-tarpaulin` or `cargo-llvm-cov` in CI | Phase 0 | Enforce coverage thresholds |
| Test helper crate or shared test fixtures | Phase 1 | The `sample_report()` helper is duplicated across `json.rs`, `markdown.rs`, `junit.rs` -- should be a shared test utility |
| `tokio::test` macro documentation | Phase 0 | Many integration tests will need `#[tokio::test]` for async. Testing.md should note this. |
| Mock trait for `MavlinkConnection` | Phase 1 | Cannot test `MissionController` logic without mocking the connection. Currently no trait to mock against. |
| `tempfile` crate for file-based tests | Phase 1 | Suite loading needs temp directories with TOML files |

---

## 7. Specific Corrections to Existing testing.md Content

### 7.1 Phase 1.2 -- "Mock mission runner"

The test says "Mock mission runner confirms each scenario is invoked in order." But `MissionController` is a concrete struct with no trait boundary. You cannot mock it without either:
- Introducing a `trait MissionExecutor` and using `impl MissionExecutor for MissionController`
- Using conditional compilation with a test double
- Using a crate like `mockall`

**Action:** Note in testing.md that this requires an architectural change before the test can be written.

### 7.2 Phase 1.4 -- Exit code 2 for zero scenarios

The current `main.rs` exits with code 1 for failures. Exit code 2 for "no scenarios" needs to be a design decision, not a test decision. Mark this as "design decision needed."

### 7.3 Phase 2.4 -- Haversine for path length

The test says "Path length computed as sum of Haversine distances between consecutive position samples." This is correct but incomplete. The existing `haversine_distance` function does not account for altitude -- it is a 2D distance. If path length should include vertical distance, you need a 3D distance function. This should be a design decision flagged in testing.md.

### 7.4 Phase 3.1 -- Backward compatibility

The test "Parse a TOML with zero fault phases -- falls back to the existing static `[fault_profile]`" is critical but the current `ScenarioFile` struct uses `faults: FaultProfile` (note: the field name is `faults`, not `fault_profile`). The testing plan uses `[fault_profile]` as the TOML key. Verify the actual key name matches, or this test will fail for the wrong reason.

### 7.5 Phase 5.2 -- Prometheus

The test "Curl `localhost:9090/metrics` during a run" is an integration test but is not labeled as SITL-gated. The metrics endpoint could be tested without SITL (just inject mock data). Clarify which tests need SITL and which do not.

### 7.6 Cross-Phase Regression -- Scenario parse check

As noted above, the command `cargo run -p px4-harness -- -s scenarios/no_faults.toml --help` does not exercise scenario parsing because `--help` exits before the scenario is loaded. Replace with a unit test that calls `ScenarioFile::load()` on each file in `scenarios/`.

---

## 8. Risk-Based Coverage Map (All Phases)

| Component/Area | Risk Rating | Risk Category | Existing Coverage | Gaps |
|---|---|---|---|---|
| `scenario.rs` parsing + validation | HIGH | Data Integrity | Weak (2 tests) | Boundary values, NaN, unknown fields, negative values |
| `fault/pipeline.rs` | HIGH | Correctness | Moderate (7 tests) | Boundary rates, replay buffer capacity, composition |
| `telemetry/processor.rs` | HIGH | Correctness | **None** | Unit conversion, bitfield parsing, system ID filtering |
| `telemetry/store.rs` | MEDIUM | Concurrency | **None** | Mutex poisoning, ordering, basic CRUD |
| `assertion/engine.rs` | HIGH | Correctness | Moderate (6 tests) | Empty telemetry, float boundaries, edge timestamps |
| `proxy/udp_proxy.rs` | HIGH | Reliability | **None** | Cannot unit-test easily (needs trait extraction or loopback test) |
| `mavlink/connection.rs` | MEDIUM | Reliability | **None** | Only testable with SITL or mock |
| `report/*.rs` | LOW | Data Integrity | Minimal (4 tests) | Adequate for current scope |
| `mission/controller.rs` | HIGH | Correctness | Weak (3 tests, haversine only) | Needs trait extraction for testability |
| Phase 1: Suite runner | HIGH | Correctness | N/A | Covered by testing.md, needs mock infra |
| Phase 2: New assertions | HIGH | Correctness | N/A | Float edge cases, missing data handling |
| Phase 3: Dynamic faults | HIGH | Correctness + Timing | N/A | Phase composition, backward compat |
| Phase 4: Multi-vehicle | MEDIUM | Correctness | N/A | Routing, system ID edge cases |
| Phase 5: Dashboard | LOW | UX | N/A | Mostly manual validation, acceptable |
| Phase 6: Fuzzing | MEDIUM | Correctness | N/A | Input validation, reproducibility |

---

## 9. Summary of Recommendations (Priority Order)

### Must Do (Before Phase 1 Development)

1. **Add a Phase 0 section** to testing.md covering the existing untested modules: `telemetry/processor.rs`, `telemetry/store.rs`, and additional edge cases for `scenario.rs` and `fault/pipeline.rs`.
2. **Add entry/exit criteria** with measurable thresholds (coverage, timing, no-unwrap policy).
3. **Fix the regression check** for scenario parsing (the `--help` flag bypasses parsing).
4. **Document the need for trait extraction** in `MissionController` and `MavlinkConnection` before Phase 1 mock-based tests can work.
5. **Add concurrency test section** for shared state components.

### Should Do (During Phase 1-3 Development)

6. Add performance test requirements per phase.
7. Add security/robustness tests for TOML parsing and UDP handling.
8. Introduce property-based testing for `haversine_distance`, `FaultProfile` serde round-trips, and fault pipeline invariants.
9. Add `proptest`, `tempfile`, and coverage tooling to test infrastructure.
10. Resolve design decisions flagged in Phase 3 (composition semantics, time reference) before writing tests.

### Nice to Have (Phase 4+)

11. Snapshot testing (`insta`) for report output stability.
12. Fuzz testing (`cargo-fuzz`) for TOML and MAVLink parsing.
13. ARM-specific `Ordering` review for atomics.
