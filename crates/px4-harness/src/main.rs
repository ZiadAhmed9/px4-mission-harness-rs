use anyhow::{Context, Result};
use clap::Parser;
use px4_harness_core::assertion::engine::{evaluate_assertions, AssertionResult};
use px4_harness_core::mavlink::connection::MavlinkConnection;
use px4_harness_core::mission::controller::MissionController;
use px4_harness_core::proxy::udp_proxy::UdpProxy;
use px4_harness_core::report::{json, junit, markdown, model::Report, model::SuiteReport};
use px4_harness_core::scenario::ScenarioFile;
use px4_harness_core::suite::SuiteFile;
use px4_harness_core::telemetry::store::TelemetryStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// PX4 Mission Harness: A tool for testing the resilience of PX4 missions.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Path to a scenario TOML file (mutually exclusive with --suite)
    #[arg(short, long, group = "input")]
    scenario: Option<PathBuf>,

    /// Path to a suite TOML file or directory of scenario TOMLs (mutually exclusive with
    /// --scenario)
    #[arg(long, group = "input")]
    suite: Option<PathBuf>,

    /// Port where PX4 SITL sends MAVLink (default: 14550)
    #[arg(long, default_value_t = 14550)]
    px4_port: u16,

    /// Port for the proxy's client side (default: 14560)
    #[arg(long, default_value_t = 14560)]
    proxy_port: u16,

    /// Enable verbose telemetry logging during mission
    #[arg(short, long)]
    verbose: bool,

    /// Write JSON report to this file
    #[arg(long)]
    json: Option<PathBuf>,

    /// Write Markdown report to this file
    #[arg(long)]
    markdown: Option<PathBuf>,

    /// Write JUnit XML report to this file
    #[arg(long)]
    junit: Option<PathBuf>,
}

/// Run a single scenario end-to-end: start proxy, connect, execute mission, evaluate assertions.
///
/// Returns the telemetry store and assertion results so the caller can build a `Report`.
/// The proxy handle is aborted before returning to free the bound ports.
async fn run_single_scenario(
    scenario: &ScenarioFile,
    px4_port: u16,
    proxy_port: u16,
    verbose: bool,
) -> Result<(Arc<TelemetryStore>, Vec<AssertionResult>)> {
    // Start the fault injection proxy.
    let proxy = UdpProxy::new(px4_port, proxy_port);
    let (proxy_addr, proxy_handle) = proxy
        .start(scenario.faults.clone(), scenario.fault_phases.clone())
        .await
        .context("failed to start proxy")?;
    println!(
        "  Proxy: PX4 ({}) -> [faults] -> harness ({})",
        px4_port, proxy_addr
    );

    // Connect to the proxy's client port.
    let conn = Arc::new(
        MavlinkConnection::connect(&format!("udpout:127.0.0.1:{}", proxy_port))
            .context("failed to connect MAVLink")?,
    );

    // Start receiving messages in background.
    let rx = conn.start_recv_task();

    // Create mission controller and run the mission.
    let controller = MissionController::new(Arc::clone(&conn));
    let store = controller
        .run_mission(&scenario.mission, rx, verbose)
        .await
        .context("mission execution failed")?;

    // Evaluate assertions against collected telemetry.
    let results = evaluate_assertions(&scenario.assertions, &scenario.mission.waypoints, &store);

    // Free the ports so the next scenario can bind them.
    proxy_handle.abort();

    Ok((store, results))
}

/// Print a per-scenario assertion summary and return (passed, failed) counts.
fn print_assertion_results(results: &[AssertionResult]) -> (usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    for r in results {
        let status = if r.passed { "PASS" } else { "FAIL" };
        println!("    [{}] {}: {}", status, r.name, r.reason);
        if r.passed {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    (passed, failed)
}

/// Run every scenario in a suite sequentially, reusing the same proxy ports.
async fn run_suite(
    suite_path: &Path,
    px4_port: u16,
    proxy_port: u16,
    verbose: bool,
) -> Result<SuiteReport> {
    // Load the suite — treat a directory as an implicit suite of all .toml files inside it.
    let suite = if suite_path.is_dir() {
        SuiteFile::from_directory(suite_path).with_context(|| {
            format!(
                "failed to build suite from directory {}",
                suite_path.display()
            )
        })?
    } else {
        SuiteFile::load(suite_path)
            .with_context(|| format!("failed to load suite file {}", suite_path.display()))?
    };

    println!("Suite: {}", suite.suite.name);
    if let Some(desc) = &suite.suite.description {
        println!("  {}", desc);
    }
    println!("  {} scenario(s) to run", suite.suite.scenarios.len());

    // Resolve scenario paths relative to the suite file's directory (or the directory itself).
    let base_dir = if suite_path.is_dir() {
        suite_path.to_path_buf()
    } else {
        suite_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };

    let scenarios = suite
        .load_scenarios(&base_dir)
        .context("failed to load scenarios listed in suite")?;

    let mut reports: Vec<Report> = Vec::with_capacity(scenarios.len());

    for (idx, (path, scenario)) in scenarios.iter().enumerate() {
        println!(
            "\n[{}/{}] {}",
            idx + 1,
            scenarios.len(),
            scenario.scenario.name
        );
        println!("  File: {}", path.display());
        println!("  Waypoints: {}", scenario.mission.waypoints.len());
        println!("  Loss rate: {:.0}%", scenario.faults.loss_rate * 100.0);

        let (store, results) = run_single_scenario(scenario, px4_port, proxy_port, verbose)
            .await
            .with_context(|| format!("scenario '{}' failed", scenario.scenario.name))?;

        let (passed, failed) = print_assertion_results(&results);
        let status = if failed == 0 { "PASS" } else { "FAIL" };
        println!("  -> [{}] {} passed, {} failed", status, passed, failed);

        let report = Report::build(scenario, &store, &results);
        reports.push(report);
    }

    let suite_report = SuiteReport::build(
        suite.suite.name.clone(),
        suite.suite.description.clone(),
        reports,
    );

    Ok(suite_report)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("PX4-Harness version: {}", px4_harness_core::version());

    match (&cli.scenario, &cli.suite) {
        // ── Single scenario mode ──────────────────────────────────────────────
        (Some(scenario_path), None) => {
            let scenario =
                ScenarioFile::load(scenario_path).context("failed to load scenario file")?;

            println!("Loaded scenario: {}", scenario.scenario.name);
            println!("  Waypoints: {}", scenario.mission.waypoints.len());
            println!("  Loss rate: {:.0}%", scenario.faults.loss_rate * 100.0);
            println!("  Assertions: {}", scenario.assertions.len());

            let (store, results) =
                run_single_scenario(&scenario, cli.px4_port, cli.proxy_port, cli.verbose).await?;

            // Print telemetry summary.
            {
                let positions = store.positions.lock().unwrap();
                let attitudes = store.attitudes.lock().unwrap();
                let statuses = store.statuses.lock().unwrap();
                println!("\nTelemetry summary:");
                println!("  Position samples: {}", positions.len());
                println!("  Attitude samples: {}", attitudes.len());
                println!("  Status samples: {}", statuses.len());
                if let Some(last) = positions.last() {
                    println!(
                        "  Final position: ({:.6}, {:.6}) alt={:.1}m",
                        last.latitude, last.longitude, last.relative_alt
                    );
                }
            }

            println!("\n=== Assertion Results ===");
            let (passed, failed) = print_assertion_results(&results);
            println!(
                "\n{} passed, {} failed, {} total",
                passed,
                failed,
                results.len()
            );

            let report = Report::build(&scenario, &store, &results);

            if let Some(path) = &cli.json {
                std::fs::write(path, json::render_json(&report))
                    .with_context(|| format!("failed to write JSON to {}", path.display()))?;
                println!("JSON report written to {}", path.display());
            }
            if let Some(path) = &cli.markdown {
                std::fs::write(path, markdown::render_markdown(&report))
                    .with_context(|| format!("failed to write Markdown to {}", path.display()))?;
                println!("Markdown report written to {}", path.display());
            }
            if let Some(path) = &cli.junit {
                std::fs::write(path, junit::render_junit(&report))
                    .with_context(|| format!("failed to write JUnit XML to {}", path.display()))?;
                println!("JUnit XML report written to {}", path.display());
            }

            if report.failed_count > 0 {
                std::process::exit(1);
            }
        }

        // ── Suite mode ────────────────────────────────────────────────────────
        (None, Some(suite_path)) => {
            let suite_report =
                run_suite(suite_path, cli.px4_port, cli.proxy_port, cli.verbose).await?;

            println!("\n=== Suite Results ===");
            let overall = if suite_report.all_passed {
                "PASS"
            } else {
                "FAIL"
            };
            println!(
                "[{}] {}/{} scenarios passed",
                overall, suite_report.passed_scenarios, suite_report.total_scenarios
            );

            if let Some(path) = &cli.json {
                std::fs::write(path, json::render_suite_json(&suite_report))
                    .with_context(|| format!("failed to write JSON to {}", path.display()))?;
                println!("JSON suite report written to {}", path.display());
            }
            if let Some(path) = &cli.markdown {
                std::fs::write(path, markdown::render_suite_markdown(&suite_report))
                    .with_context(|| format!("failed to write Markdown to {}", path.display()))?;
                println!("Markdown suite report written to {}", path.display());
            }
            if let Some(path) = &cli.junit {
                std::fs::write(path, junit::render_suite_junit(&suite_report))
                    .with_context(|| format!("failed to write JUnit XML to {}", path.display()))?;
                println!("JUnit XML suite report written to {}", path.display());
            }

            if !suite_report.all_passed {
                std::process::exit(1);
            }
        }

        // ── Neither provided ──────────────────────────────────────────────────
        (None, None) => {
            eprintln!("error: must provide either --scenario <FILE> or --suite <FILE|DIR>");
            eprintln!("       Run with --help for usage.");
            std::process::exit(2);
        }

        // Clap `group = "input"` prevents both being set simultaneously; this arm is
        // unreachable at runtime but required for exhaustive matching.
        (Some(_), Some(_)) => unreachable!("clap group ensures mutual exclusion"),
    }

    Ok(())
}
