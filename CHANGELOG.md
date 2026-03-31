# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-03-31

### Added

- TOML-based scenario definition with mission, fault profile, and assertion configuration
- MAVLink connection to PX4 SITL with heartbeat handling
- Mission controller supporting arm, takeoff, waypoint navigation, and landing
- Telemetry collection for position, attitude, and vehicle status
- UDP proxy sitting between PX4 SITL and GCS for transparent fault injection
- Fault injection profiles: fixed delay, jitter, packet loss, burst loss, duplication, stale replay
- Assertion engine: waypoint reached (with tolerance), altitude checks, landing verification
- Report generation in JSON, Markdown, and JUnit XML formats
- Multi-scenario suite runner
- Scenario generation from templates
- CLI with configurable ports, verbosity, and output options
