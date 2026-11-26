use futures_util::future::{select, Either};
use std::future::Future;
use std::time::Duration;

pub(crate) const NETWORK_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const DIAGNOSTIC_BUDGET: Duration = Duration::from_secs(2);

/// Poll all diagnostic work while emitting bounded progress. The timer is
/// dropped with the work, so a finished/cancelled run leaves no background task.
pub(crate) async fn run_diagnostic_probes<D: Future, I: Future, G: Future>(
    deadline: tokio::time::Instant,
    dns: D,
    internet: I,
    gateway: G,
    mut progress: impl FnMut(u8),
) -> (D::Output, I::Output, G::Output) {
    let mut work = Box::pin(futures_util::future::join3(dns, internet, gateway));
    progress(42);
    loop {
        match select(
            work,
            Box::pin(tokio::time::sleep(Duration::from_millis(100))),
        )
        .await
        {
            Either::Left((result, _)) => return result,
            Either::Right((_, pending)) => {
                work = pending;
                let elapsed = DIAGNOSTIC_BUDGET.saturating_sub(
                    deadline.saturating_duration_since(tokio::time::Instant::now()),
                );
                let fraction = (elapsed.as_millis() * 54 / DIAGNOSTIC_BUDGET.as_millis()).min(54);
                progress(42 + fraction as u8);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn diagnostics_start_together_and_advance_while_waiting() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let started = AtomicUsize::new(0);
            let release = tokio::sync::Semaphore::new(0);
            let task = |value| {
                let started = &started;
                let release = &release;
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    release.acquire().await.unwrap().forget();
                    value
                }
            };
            let mut progress = Vec::new();
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                run_diagnostic_probes(
                    tokio::time::Instant::now() + DIAGNOSTIC_BUDGET,
                    task(1),
                    task(2),
                    task(3),
                    |percent| {
                        progress.push(percent);
                        if percent > 42 && started.load(Ordering::SeqCst) == 3 {
                            release.add_permits(3);
                        }
                    },
                ),
            )
            .await
            .unwrap();
            assert_eq!(result, (1, 2, 3));
            assert!(progress.len() > 1);
            assert!(progress.windows(2).all(|pair| pair[1] >= pair[0]));
            assert!(progress.iter().all(|percent| *percent < 100));
        });
    }
}
