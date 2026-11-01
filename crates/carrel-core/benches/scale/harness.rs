//! Timing, the results file and the comparison with an earlier run.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How a run was asked for.
#[derive(Debug, Clone)]
pub struct Plan {
    /// `cargo test`'s run: the smallest size once, to check every benchmark still works.
    pub smoke: bool,
    /// Only the benchmarks whose name contains this.
    pub filter: Option<String>,
    /// Sizes to use instead of each benchmark's own, from `CARREL_SCALE_SIZES`.
    pub sizes: Option<Vec<usize>>,
    /// The largest size to run, from `CARREL_SCALE_MAX`.
    pub max: usize,
    /// How long to keep sampling one measurement.
    pub budget: Duration,
}

impl Plan {
    /// The plan for this process's arguments and environment. `cargo bench` passes `--bench`;
    /// `cargo test` runs the benchmark with no arguments, as a smoke test.
    pub fn from_env() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let smoke = !args.iter().any(|a| a == "--bench");
        let filter = args.into_iter().find(|a| !a.starts_with('-'));
        let sizes = std::env::var("CARREL_SCALE_SIZES").ok().map(|text| {
            text.split(',')
                .filter_map(|n| n.trim().replace('_', "").parse().ok())
                .collect()
        });
        let max = std::env::var("CARREL_SCALE_MAX")
            .ok()
            .and_then(|n| n.replace('_', "").parse().ok())
            .unwrap_or(usize::MAX);
        let budget = std::env::var("CARREL_SCALE_SECONDS")
            .ok()
            .and_then(|n| n.parse().ok())
            .map_or(Duration::from_secs(3), Duration::from_secs_f64);
        Self {
            smoke,
            filter,
            sizes,
            max,
            budget,
        }
    }

    /// Whether to run the benchmark called `name`.
    pub fn wants(&self, name: &str) -> bool {
        self.filter.as_deref().is_none_or(|f| name.contains(f))
    }

    /// The sizes to run a benchmark at, given its own.
    pub fn sizes(&self, own: &[usize]) -> Vec<usize> {
        let mut sizes: Vec<usize> = self.sizes.clone().unwrap_or_else(|| own.to_vec());
        sizes.retain(|n| *n <= self.max);
        sizes.sort_unstable();
        if self.smoke {
            sizes.truncate(1);
            // A smoke run checks the code works, so it keeps to small fixtures.
            return sizes.into_iter().map(|n| n.min(1_000)).collect();
        }
        sizes
    }
}

/// One benchmark at one size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    /// What was measured, such as `manifest_diff`.
    pub name: String,
    /// How many items the fixture had: cards, files, documents, vectors or commits.
    pub size: usize,
    /// The median time of one run over the whole fixture, in nanoseconds.
    pub median_ns: f64,
    /// The fastest run, in nanoseconds.
    pub min_ns: f64,
    /// How many runs were timed.
    pub samples: usize,
}

impl Measurement {
    /// The key a measurement is compared by.
    fn key(&self) -> (String, usize) {
        (self.name.clone(), self.size)
    }
}

/// Times `run` over a fixture of `size` items: once to warm up, then until the plan's budget is
/// spent, at least three and at most fifty times (once in a smoke run).
pub fn measure<T>(plan: &Plan, name: &str, size: usize, mut run: impl FnMut() -> T) -> Measurement {
    std::hint::black_box(run());
    let mut times = Vec::new();
    let started = Instant::now();
    loop {
        let began = Instant::now();
        std::hint::black_box(run());
        times.push(began.elapsed().as_secs_f64() * 1e9);
        let spent = started.elapsed();
        if plan.smoke || times.len() >= 50 || (times.len() >= 3 && spent >= plan.budget) {
            break;
        }
    }
    times.sort_by(f64::total_cmp);
    let measurement = Measurement {
        name: name.to_owned(),
        size,
        median_ns: times[times.len() / 2],
        min_ns: times[0],
        samples: times.len(),
    };
    eprintln!(
        "{name:<24} {size:>9}  {:>12}  {:>10} each  ({} runs)",
        duration(measurement.median_ns),
        duration(per_item(&measurement)),
        measurement.samples
    );
    measurement
}

/// The time per item, in nanoseconds.
pub fn per_item(m: &Measurement) -> f64 {
    #[allow(clippy::cast_precision_loss)] // sizes far below 2^52
    let size = m.size.max(1) as f64;
    m.median_ns / size
}

/// A time in nanoseconds, in the largest unit that keeps it above 1.
pub fn duration(ns: f64) -> String {
    if ns >= 1e9 {
        format!("{:.2} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e3 {
        format!("{:.2} µs", ns / 1e3)
    } else {
        format!("{ns:.0} ns")
    }
}

/// A run's results, as written to the results file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Results {
    /// The commit measured, from `GITHUB_SHA` when CI runs it.
    #[serde(default)]
    pub commit: String,
    /// Every measurement.
    pub measurements: Vec<Measurement>,
}

/// Reads an earlier run's results, or `None` if there are none to read.
pub fn read_results(path: &Path) -> Option<Results> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(results) => Some(results),
        Err(e) => {
            eprintln!(
                "{}: not a results file ({e}); comparing with nothing",
                path.display()
            );
            None
        }
    }
}

/// A Markdown report of `results`, compared with `baseline` if there is one: each measurement's
/// time, and its change, with any slower by more than `threshold` percent flagged. Returns the
/// report and how many were flagged.
pub fn report(results: &Results, baseline: Option<&Results>, threshold: f64) -> (String, usize) {
    let mut out = String::from("## Performance benchmarks\n\n");
    match baseline {
        Some(b) if !b.commit.is_empty() => {
            let _ = writeln!(
                out,
                "Compared with `{}`; slower by more than {threshold}% is flagged.\n",
                short(&b.commit)
            );
        }
        Some(_) => {
            let _ = writeln!(
                out,
                "Compared with the previous run; slower by more than {threshold}% is flagged.\n"
            );
        }
        None => out.push_str("No earlier run to compare with.\n\n"),
    }
    out.push_str(
        "| Benchmark | Size | Time | Per item | Change |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    let mut flagged = 0;
    for m in &results.measurements {
        let before = baseline.and_then(|b| b.measurements.iter().find(|o| o.key() == m.key()));
        let change = match before {
            Some(o) if o.median_ns > 0.0 => {
                let percent = (m.median_ns / o.median_ns - 1.0) * 100.0;
                if percent > threshold {
                    flagged += 1;
                    format!("**+{percent:.0}%** ⚠️")
                } else {
                    format!("{percent:+.0}%")
                }
            }
            _ => "new".to_owned(),
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {change} |",
            m.name,
            m.size,
            duration(m.median_ns),
            duration(per_item(m))
        );
    }
    if flagged > 0 {
        let _ = writeln!(
            out,
            "\n{flagged} slower by more than {threshold}%. Shared runners vary by 10–20% from run to run, so rerun before chasing one."
        );
    }
    (out, flagged)
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// A small, fast, seeded random number generator (xorshift64*), so fixtures are the same on
/// every run without a crate for it.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator from `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `n`.
    pub fn below(&mut self, n: usize) -> usize {
        #[allow(clippy::cast_possible_truncation)] // the result is below n, a usize
        let r = (self.next_u64() % n as u64) as usize;
        r
    }

    /// A number in [-1, 1).
    pub fn unit(&mut self) -> f32 {
        #[allow(clippy::cast_precision_loss)] // 24 bits fit an f32 exactly
        let r = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        r * 2.0 - 1.0
    }

    /// An index below `n`, favouring small ones as word frequencies do.
    pub fn skewed(&mut self, n: usize) -> usize {
        let a = self.below(n);
        let b = self.below(n);
        a.min(b).min(self.below(n))
    }
}
