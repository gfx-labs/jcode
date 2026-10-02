//! Fire N concurrent OpenJev memory decisions through the real shared limiter.
//! Run several copies at once to verify cross-process pacing:
//! `cargo run -p jcode-base --example openjev_burst -- 15`
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    unsafe { std::env::set_var("JCODE_MEMORY_JEV_PROVIDER", "openjev") };
    let client = jcode_base::jev::JevClient::new()?;
    let question = serde_json::json!({"relevant": {"type": "noul", "instructions": "Is this about terminals?"}})
        .as_object()
        .unwrap()
        .clone();
    let started = std::time::Instant::now();
    let results = futures::future::join_all((0..n).map(|_| {
        client.evaluate(
            serde_json::json!({"task": "fix tty restore"}),
            question.clone(),
        )
    }))
    .await;
    let ok = results.iter().filter(|r| r.is_ok()).count();
    for error in results.iter().filter_map(|r| r.as_ref().err()) {
        eprintln!("error: {error:#}");
    }
    println!(
        "pid={} ok={ok}/{n} elapsed={:.2}s",
        std::process::id(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
