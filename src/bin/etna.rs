// ETNA workload runner for lz4_flex.
//
// Usage: cargo run --release --bin etna -- <tool> <property>
//   tool:     etna | proptest | quickcheck | crabcheck | hegel
//   property: DontPanicOnDrop | SmallDictNoPanic | DecompressShortInputNoPanic | All
//
// Each invocation prints exactly one JSON line on stdout:
//   {status, tests, discards, time, counterexample, error, tool, property}
// Exit status is always 0 except for argument parse errors (exit 2).

use crabcheck::quickcheck as crabcheck_qc;
use hegel::{generators as hgen, HealthCheck, Hegel, Settings as HegelSettings, TestCase};
use lz4_flex::etna::{
    property_decompress_short_input_no_panic, property_dont_panic_on_drop,
    property_small_dict_no_panic, PropertyResult,
};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestCaseError, TestRunner};
use quickcheck::{Arbitrary as QcArbitrary, Gen, QuickCheck, ResultStatus, TestResult};
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct Metrics {
    inputs: u64,
    elapsed_us: u128,
}

impl Metrics {
    fn combine(self, other: Metrics) -> Metrics {
        Metrics {
            inputs: self.inputs + other.inputs,
            elapsed_us: self.elapsed_us + other.elapsed_us,
        }
    }
}

type Outcome = (Result<(), String>, Metrics);

fn to_err(r: PropertyResult) -> Result<(), String> {
    match r {
        PropertyResult::Pass | PropertyResult::Discard => Ok(()),
        PropertyResult::Fail(m) => Err(m),
    }
}

const ALL_PROPERTIES: &[&str] =
    &["DontPanicOnDrop", "SmallDictNoPanic", "DecompressShortInputNoPanic"];

fn run_all<F: FnMut(&str) -> Outcome>(mut f: F) -> Outcome {
    let mut total = Metrics::default();
    let mut final_status: Result<(), String> = Ok(());
    for p in ALL_PROPERTIES {
        let (r, m) = f(p);
        total = total.combine(m);
        if r.is_err() && final_status.is_ok() {
            final_status = r;
        }
    }
    (final_status, total)
}

// Cap on generated input/dict sizes. The roundtrip property allocates at
// least input.len() + capacity for compressed output; keep both small.
const MAX_INPUT_LEN: usize = 256;
const MAX_DICT_LEN: usize = 16;
const MAX_SHORT_INPUT_LEN: usize = 8; // for decompress_short_input

// ============================================================================
// Input wrappers
// ============================================================================

#[derive(Clone)]
struct BytesInput {
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct InputDictInput {
    input: Vec<u8>,
    dict: Vec<u8>,
}

#[derive(Clone)]
struct ShortBytesInput {
    bytes: Vec<u8>,
}

impl fmt::Debug for BytesInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.bytes)
    }
}
impl fmt::Display for BytesInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Debug for InputDictInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?}", self.input, self.dict)
    }
}
impl fmt::Display for InputDictInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Debug for ShortBytesInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.bytes)
    }
}
impl fmt::Display for ShortBytesInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

// ============================================================================
// etna (deterministic witness-shaped inputs)
// ============================================================================

fn run_etna_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_etna_property);
    }
    let t0 = Instant::now();
    let result = match property {
        "DontPanicOnDrop" => to_err(property_dont_panic_on_drop(b"hello world".to_vec())),
        "SmallDictNoPanic" => to_err(property_small_dict_no_panic((
            vec![10u8, 12, 14, 16, 18, 10, 12, 14, 16, 18, 10, 12, 14, 16, 18, 10, 12, 14, 16, 18],
            vec![10u8, 12, 14],
        ))),
        "DecompressShortInputNoPanic" => {
            to_err(property_decompress_short_input_no_panic(Vec::new()))
        }
        _ => {
            return (
                Err(format!("Unknown property for etna: {property}")),
                Metrics::default(),
            )
        }
    };
    let elapsed_us = t0.elapsed().as_micros();
    (result, Metrics { inputs: 1, elapsed_us })
}

// ============================================================================
// proptest
// ============================================================================

fn bytes_strategy() -> BoxedStrategy<BytesInput> {
    prop::collection::vec(any::<u8>(), 0..=MAX_INPUT_LEN)
        .prop_map(|bytes| BytesInput { bytes })
        .boxed()
}

fn input_dict_strategy() -> BoxedStrategy<InputDictInput> {
    (
        prop::collection::vec(any::<u8>(), 0..=MAX_INPUT_LEN),
        prop::collection::vec(any::<u8>(), 0..=MAX_DICT_LEN),
    )
        .prop_map(|(input, dict)| InputDictInput { input, dict })
        .boxed()
}

fn short_bytes_strategy() -> BoxedStrategy<ShortBytesInput> {
    prop::collection::vec(any::<u8>(), 0..=MAX_SHORT_INPUT_LEN)
        .prop_map(|bytes| ShortBytesInput { bytes })
        .boxed()
}

fn run_proptest_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_proptest_property);
    }
    let counter = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let mut runner = TestRunner::new(ProptestConfig::default());
    let c = counter.clone();
    let result: Result<(), String> = match property {
        "DontPanicOnDrop" => runner
            .run(&bytes_strategy(), move |args| {
                c.fetch_add(1, Ordering::Relaxed);
                let cex = format!("({:?})", args);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_dont_panic_on_drop(args.bytes.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => Ok(()),
                    Ok(PropertyResult::Fail(_)) | Err(_) => Err(TestCaseError::fail(cex)),
                }
            })
            .map_err(|e| match e {
                proptest::test_runner::TestError::Fail(r, _) => r.to_string(),
                other => other.to_string(),
            }),
        "SmallDictNoPanic" => runner
            .run(&input_dict_strategy(), move |args| {
                c.fetch_add(1, Ordering::Relaxed);
                let cex = format!("({:?})", args);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_small_dict_no_panic((args.input.clone(), args.dict.clone()))
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => Ok(()),
                    Ok(PropertyResult::Fail(_)) | Err(_) => Err(TestCaseError::fail(cex)),
                }
            })
            .map_err(|e| match e {
                proptest::test_runner::TestError::Fail(r, _) => r.to_string(),
                other => other.to_string(),
            }),
        "DecompressShortInputNoPanic" => runner
            .run(&short_bytes_strategy(), move |args| {
                c.fetch_add(1, Ordering::Relaxed);
                let cex = format!("({:?})", args);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_decompress_short_input_no_panic(args.bytes.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => Ok(()),
                    Ok(PropertyResult::Fail(_)) | Err(_) => Err(TestCaseError::fail(cex)),
                }
            })
            .map_err(|e| match e {
                proptest::test_runner::TestError::Fail(r, _) => r.to_string(),
                other => other.to_string(),
            }),
        _ => {
            return (
                Err(format!("Unknown property for proptest: {property}")),
                Metrics::default(),
            )
        }
    };
    let elapsed_us = t0.elapsed().as_micros();
    let inputs = counter.load(Ordering::Relaxed);
    (result, Metrics { inputs, elapsed_us })
}

// ============================================================================
// quickcheck (forked, fn-pointer based)
// ============================================================================

impl QcArbitrary for BytesInput {
    fn arbitrary(g: &mut Gen) -> Self {
        let len = (<u8 as QcArbitrary>::arbitrary(g) as usize) % (MAX_INPUT_LEN + 1);
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(<u8 as QcArbitrary>::arbitrary(g));
        }
        BytesInput { bytes }
    }
}

impl QcArbitrary for InputDictInput {
    fn arbitrary(g: &mut Gen) -> Self {
        let ilen = (<u8 as QcArbitrary>::arbitrary(g) as usize) % (MAX_INPUT_LEN + 1);
        let dlen = (<u8 as QcArbitrary>::arbitrary(g) as usize) % (MAX_DICT_LEN + 1);
        let mut input = Vec::with_capacity(ilen);
        for _ in 0..ilen {
            input.push(<u8 as QcArbitrary>::arbitrary(g));
        }
        let mut dict = Vec::with_capacity(dlen);
        for _ in 0..dlen {
            dict.push(<u8 as QcArbitrary>::arbitrary(g));
        }
        InputDictInput { input, dict }
    }
}

impl QcArbitrary for ShortBytesInput {
    fn arbitrary(g: &mut Gen) -> Self {
        let len = (<u8 as QcArbitrary>::arbitrary(g) as usize) % (MAX_SHORT_INPUT_LEN + 1);
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(<u8 as QcArbitrary>::arbitrary(g));
        }
        ShortBytesInput { bytes }
    }
}

static QC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn qc_run<F>(prop: F) -> TestResult
where
    F: FnOnce() -> PropertyResult + std::panic::UnwindSafe,
{
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let res = std::panic::catch_unwind(prop);
    match res {
        Ok(PropertyResult::Pass) => TestResult::passed(),
        Ok(PropertyResult::Discard) => TestResult::discard(),
        Ok(PropertyResult::Fail(_)) | Err(_) => TestResult::failed(),
    }
}

fn qc_dont_panic_on_drop(args: BytesInput) -> TestResult {
    qc_run(move || property_dont_panic_on_drop(args.bytes))
}

fn qc_small_dict_no_panic(args: InputDictInput) -> TestResult {
    qc_run(move || property_small_dict_no_panic((args.input, args.dict)))
}

fn qc_decompress_short_input_no_panic(args: ShortBytesInput) -> TestResult {
    qc_run(move || property_decompress_short_input_no_panic(args.bytes))
}

fn run_quickcheck_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_quickcheck_property);
    }
    QC_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let result = match property {
        "DontPanicOnDrop" => QuickCheck::new()
            .tests(200)
            .max_tests(2000)
            .max_time(Duration::from_secs(86_400))
            .quicktest(qc_dont_panic_on_drop as fn(BytesInput) -> TestResult),
        "SmallDictNoPanic" => QuickCheck::new()
            .tests(200)
            .max_tests(2000)
            .max_time(Duration::from_secs(86_400))
            .quicktest(qc_small_dict_no_panic as fn(InputDictInput) -> TestResult),
        "DecompressShortInputNoPanic" => QuickCheck::new()
            .tests(200)
            .max_tests(2000)
            .max_time(Duration::from_secs(86_400))
            .quicktest(qc_decompress_short_input_no_panic as fn(ShortBytesInput) -> TestResult),
        _ => {
            return (
                Err(format!("Unknown property for quickcheck: {property}")),
                Metrics::default(),
            )
        }
    };
    let elapsed_us = t0.elapsed().as_micros();
    let inputs = QC_COUNTER.load(Ordering::Relaxed);
    let metrics = Metrics { inputs, elapsed_us };
    let status = match result.status {
        ResultStatus::Finished => Ok(()),
        ResultStatus::Failed { arguments } => Err(format!("({})", arguments.join(" "))),
        ResultStatus::Aborted { err } => Err(format!("aborted: {err:?}")),
        ResultStatus::TimedOut => Err("timed out".to_string()),
        ResultStatus::GaveUp => Err(format!(
            "gave up: passed={}, discarded={}",
            result.n_tests_passed, result.n_tests_discarded
        )),
    };
    (status, metrics)
}

// ============================================================================
// crabcheck
// ============================================================================

use crabcheck::quickcheck::Arbitrary as CcArbitrary;
use rand::Rng as CcRng;

impl<R: CcRng> CcArbitrary<R> for BytesInput {
    fn generate(rng: &mut R, _n: usize) -> Self {
        let len = (rng.random::<u8>() as usize) % (MAX_INPUT_LEN + 1);
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(rng.random::<u8>());
        }
        BytesInput { bytes }
    }
}

impl<R: CcRng> CcArbitrary<R> for InputDictInput {
    fn generate(rng: &mut R, _n: usize) -> Self {
        let ilen = (rng.random::<u8>() as usize) % (MAX_INPUT_LEN + 1);
        let dlen = (rng.random::<u8>() as usize) % (MAX_DICT_LEN + 1);
        let mut input = Vec::with_capacity(ilen);
        for _ in 0..ilen {
            input.push(rng.random::<u8>());
        }
        let mut dict = Vec::with_capacity(dlen);
        for _ in 0..dlen {
            dict.push(rng.random::<u8>());
        }
        InputDictInput { input, dict }
    }
}

impl<R: CcRng> CcArbitrary<R> for ShortBytesInput {
    fn generate(rng: &mut R, _n: usize) -> Self {
        let len = (rng.random::<u8>() as usize) % (MAX_SHORT_INPUT_LEN + 1);
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(rng.random::<u8>());
        }
        ShortBytesInput { bytes }
    }
}

static CC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn cc_dont_panic_on_drop(v: BytesInput) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    match property_dont_panic_on_drop(v.bytes) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_small_dict_no_panic(v: InputDictInput) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    match property_small_dict_no_panic((v.input, v.dict)) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_decompress_short_input_no_panic(v: ShortBytesInput) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    match property_decompress_short_input_no_panic(v.bytes) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn run_crabcheck_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_crabcheck_property);
    }
    CC_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let cfg = crabcheck_qc::Config { tests: 200 };
    let result = match property {
        "DontPanicOnDrop" => crabcheck_qc::quickcheck_with_config(
            cfg,
            cc_dont_panic_on_drop as fn(BytesInput) -> Option<bool>,
        ),
        "SmallDictNoPanic" => crabcheck_qc::quickcheck_with_config(
            cfg,
            cc_small_dict_no_panic as fn(InputDictInput) -> Option<bool>,
        ),
        "DecompressShortInputNoPanic" => crabcheck_qc::quickcheck_with_config(
            cfg,
            cc_decompress_short_input_no_panic as fn(ShortBytesInput) -> Option<bool>,
        ),
        _ => {
            return (
                Err(format!("Unknown property for crabcheck: {property}")),
                Metrics::default(),
            )
        }
    };
    let elapsed_us = t0.elapsed().as_micros();
    let inputs = CC_COUNTER.load(Ordering::Relaxed);
    let metrics = Metrics { inputs, elapsed_us };
    let status = match result.status {
        crabcheck_qc::ResultStatus::Finished => Ok(()),
        crabcheck_qc::ResultStatus::Failed { arguments } => {
            Err(format!("({})", arguments.join(" ")))
        }
        crabcheck_qc::ResultStatus::TimedOut => Err("timed out".to_string()),
        crabcheck_qc::ResultStatus::GaveUp => Err(format!(
            "gave up: passed={}, discarded={}",
            result.passed, result.discarded
        )),
        crabcheck_qc::ResultStatus::Aborted { error } => Err(format!("aborted: {error}")),
    };
    (status, metrics)
}

// ============================================================================
// hegel
// ============================================================================

static HG_COUNTER: AtomicU64 = AtomicU64::new(0);

fn hegel_settings() -> HegelSettings {
    HegelSettings::new()
        .test_cases(200)
        .suppress_health_check(HealthCheck::all())
}

fn hg_draw_byte(tc: &TestCase) -> u8 {
    tc.draw(hgen::integers::<u32>().min_value(0).max_value(255)) as u8
}

fn hg_draw_bytes(tc: &TestCase, max: usize) -> Vec<u8> {
    let len = (hg_draw_byte(tc) as usize) % (max + 1);
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(hg_draw_byte(tc));
    }
    bytes
}

fn run_hegel_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_hegel_property);
    }
    HG_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let settings = hegel_settings();
    let run_result = std::panic::catch_unwind(AssertUnwindSafe(|| match property {
        "DontPanicOnDrop" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let bytes = hg_draw_bytes(&tc, MAX_INPUT_LEN);
                let cex = format!("({:?})", bytes);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_dont_panic_on_drop(bytes.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    Ok(PropertyResult::Fail(_)) | Err(_) => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "SmallDictNoPanic" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let input = hg_draw_bytes(&tc, MAX_INPUT_LEN);
                let dict = hg_draw_bytes(&tc, MAX_DICT_LEN);
                let cex = format!("({:?} {:?})", input, dict);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_small_dict_no_panic((input.clone(), dict.clone()))
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    Ok(PropertyResult::Fail(_)) | Err(_) => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "DecompressShortInputNoPanic" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let bytes = hg_draw_bytes(&tc, MAX_SHORT_INPUT_LEN);
                let cex = format!("({:?})", bytes);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_decompress_short_input_no_panic(bytes.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    Ok(PropertyResult::Fail(_)) | Err(_) => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        _ => panic!("__unknown_property:{property}"),
    }));
    let elapsed_us = t0.elapsed().as_micros();
    let inputs = HG_COUNTER.load(Ordering::Relaxed);
    let metrics = Metrics { inputs, elapsed_us };
    let status = match run_result {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "hegel panicked with non-string payload".to_string()
            };
            if let Some(rest) = msg.strip_prefix("__unknown_property:") {
                return (
                    Err(format!("Unknown property for hegel: {rest}")),
                    Metrics::default(),
                );
            }
            Err(msg
                .strip_prefix("Property test failed: ")
                .unwrap_or(&msg)
                .to_string())
        }
    };
    (status, metrics)
}

// ============================================================================
// dispatch + main
// ============================================================================

fn run(tool: &str, property: &str) -> Outcome {
    match tool {
        "etna" => run_etna_property(property),
        "proptest" => run_proptest_property(property),
        "quickcheck" => run_quickcheck_property(property),
        "crabcheck" => run_crabcheck_property(property),
        "hegel" => run_hegel_property(property),
        _ => (Err(format!("Unknown tool: {tool}")), Metrics::default()),
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn emit_json(
    tool: &str,
    property: &str,
    status: &str,
    metrics: Metrics,
    counterexample: Option<&str>,
    error: Option<&str>,
) {
    let cex = counterexample.map_or("null".to_string(), json_str);
    let err = error.map_or("null".to_string(), json_str);
    println!(
        "{{\"status\":{},\"tests\":{},\"discards\":0,\"time\":{},\"counterexample\":{},\"error\":{},\"tool\":{},\"property\":{}}}",
        json_str(status),
        metrics.inputs,
        json_str(&format!("{}us", metrics.elapsed_us)),
        cex,
        err,
        json_str(tool),
        json_str(property),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <tool> <property>", args[0]);
        eprintln!("Tools: etna | proptest | quickcheck | crabcheck | hegel");
        eprintln!(
            "Properties: DontPanicOnDrop | SmallDictNoPanic | DecompressShortInputNoPanic | All"
        );
        std::process::exit(2);
    }
    let (tool, property) = (args[1].as_str(), args[2].as_str());

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(AssertUnwindSafe(|| run(tool, property)));
    std::panic::set_hook(previous_hook);

    let (result, metrics) = match caught {
        Ok(outcome) => outcome,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "panic with non-string payload".to_string()
            };
            emit_json(
                tool,
                property,
                "aborted",
                Metrics::default(),
                None,
                Some(&format!("adapter panic: {msg}")),
            );
            return;
        }
    };

    match result {
        Ok(()) => emit_json(tool, property, "passed", metrics, None, None),
        Err(msg) => emit_json(tool, property, "failed", metrics, Some(&msg), None),
    }
}
