# PX4 Mission Harness — Next Steps Roadmap

## Phase 1 — Multi-Mission & Scenario Suites

**Goal:** Run multiple scenarios in one invocation, compare results across fault profiles.

- **Scenario suite runner** — accept a directory of TOML files or a `suite.toml` that lists multiple scenarios. Run them sequentially (or in parallel if multiple SITL instances are available).
- **Comparative report** — a summary table/matrix showing how the same mission performs under different fault profiles (e.g., "no faults vs 30% loss vs 500ms delay").
- **Exit code semantics** — exit 0 only if all scenarios in the suite pass, with per-scenario status in the report.

**Why first:** This is the highest-leverage feature — right now each run tests one scenario. Batch execution makes this a real regression/CI tool.

---

## Phase 2 — Richer Assertions & Metrics

**Goal:** Catch subtler failures and quantify mission quality, not just pass/fail.

- **Timing assertions** — "waypoint N reached within X seconds of waypoint N-1" (flight segment timing).
- **Geofence assertion** — "vehicle never exceeds bounding box / max altitude / max distance from path."
- **Velocity/attitude assertions** — "max tilt never exceeds X degrees", "ground speed stayed below Y m/s."
- **Quantitative metrics in report** — total flight time, path length, energy proxy (integral of velocity), max deviation from planned path. These aren't pass/fail but help characterize degradation.

**Why second:** The telemetry store already collects all the data needed — this phase extracts more value from it.

---

## Phase 3 — Dynamic Fault Injection (Time-Based Profiles)

**Goal:** Apply faults that change during the mission, not just static rates.

- **Phased fault profiles** — define fault parameters per mission phase (e.g., "0% loss during takeoff, 40% loss during cruise, 0% during landing").
- **Time-triggered faults** — "at T+30s, inject 500ms delay for 10 seconds" (transient spikes).
- **Event-triggered faults** — "when vehicle reaches waypoint 2, start 20% packet loss" (ties faults to mission progress).

Example TOML extension:
```toml
[[fault_phases]]
trigger = { type = "time", after_secs = 30 }
duration_secs = 15
loss_rate = 0.4
delay_ms = 200
```

**Why third:** Static fault profiles are a good start but real-world link degradation is bursty and situational. This makes test scenarios much more realistic.

---

## Phase 4 — Multi-Vehicle Support

**Goal:** Test missions involving more than one drone.

- **Per-vehicle config** — each vehicle gets its own MAVLink system ID, UDP port pair, and fault profile.
- **Proxy multiplexing** — route packets by system ID through independent fault pipelines.
- **Inter-vehicle assertions** — "vehicle 1 and vehicle 2 maintain minimum separation of X meters."

**Why here:** PX4 SITL supports multi-vehicle. This opens up swarm and formation testing, which is where communication resilience matters most.

---

## Phase 5 — Live Dashboard & Observability

**Goal:** Real-time visibility into what's happening during a test run.

- **TUI dashboard** (using `ratatui`) — show live position, fault stats (packets dropped/delayed/duplicated), assertion progress, and mission state.
- **Prometheus metrics export** — expose counters and gauges so Grafana can visualize test runs.
- **Event log** — structured log of every significant event (arm, takeoff, waypoint reached, fault applied) with timestamps, exportable as JSON.

**Why here:** As scenarios get longer and more complex (phases 1-4), visibility into what's happening during the run becomes essential, not just after.

---

## Phase 6 — Scenario Generation & Fuzzing

**Goal:** Automatically discover failure boundaries instead of hand-writing every scenario.

- **Parameter sweep** — "run this mission with loss_rate from 0% to 50% in 5% increments" — auto-generates scenarios and produces a degradation curve.
- **Binary search for failure threshold** — "find the minimum delay_ms where waypoint 2 is no longer reached within tolerance."
- **Random fault fuzzing** — randomize fault parameters within bounds across many runs, report which combinations cause failures.

**Why last:** This is the most ambitious feature but also the most powerful — it turns the harness from "verify known scenarios" into "discover unknown failure modes."

---

## Priority Summary

| Phase | Effort | Impact |
|-------|--------|--------|
| 1 — Suite runner | Medium | High (unlocks CI use) |
| 2 — Richer assertions | Low-Medium | High (leverages existing telemetry) |
| 3 — Dynamic faults | Medium | High (realistic scenarios) |
| 4 — Multi-vehicle | High | Medium (niche but powerful) |
| 5 — Dashboard | Medium | Medium (DX/observability) |
| 6 — Fuzzing | High | Very High (but depends on 1) |
