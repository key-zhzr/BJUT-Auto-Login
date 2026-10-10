use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const BACKGROUND_MESSAGE: &str =
    "App 已进入后台，计费系统请求已停止；返回前台后可重新发起";

pub(crate) fn ensure_foreground(is_in_background: &AtomicBool) -> Result<(), String> {
    if is_in_background.load(Ordering::SeqCst) {
        Err(BACKGROUND_MESSAGE.to_string())
    } else {
        Ok(())
    }
}

pub(crate) async fn wait_for_background(is_in_background: &AtomicBool) {
    loop {
        if is_in_background.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

pub(crate) fn run_read_while_foreground<'a, T, F>(
    is_in_background: &'a AtomicBool,
    future: F,
) -> impl std::future::Future<Output = Result<T, String>> + 'a
where
    F: std::future::Future<Output = Result<T, String>> + 'a,
{
    // Keep nested billing state on the heap before the caller builds its
    // command future. Stack pinning still embeds the whole request by value.
    let future = Box::pin(future);
    async move {
        ensure_foreground(is_in_background)?;
        let background = wait_for_background(is_in_background);
        futures_util::pin_mut!(background);
        match futures_util::future::select(future, background).await {
            futures_util::future::Either::Left((result, _)) => result,
            futures_util::future::Either::Right((_, _)) => Err(BACKGROUND_MESSAGE.to_string()),
        }
    }
}

pub(crate) fn run_mutation_to_completion<'a, T, F>(
    is_in_background: &'a AtomicBool,
    future: F,
) -> impl std::future::Future<Output = Result<T, String>> + 'a
where
    F: std::future::Future<Output = Result<T, String>> + 'a,
{
    let future = Box::pin(future);
    async move {
        // Reject writes only before polling starts. Dropping an in-flight write
        // when the app loses focus can leave the server result unknown and make a
        // duplicate submission look safe.
        ensure_foreground(is_in_background)?;
        future.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn a_background_app_does_not_start_a_read_or_write() {
        runtime().block_on(async {
            let background = AtomicBool::new(true);
            let polled = AtomicBool::new(false);
            let operation = || async {
                polled.store(true, Ordering::SeqCst);
                Ok(())
            };
            assert_eq!(
                run_read_while_foreground(&background, operation()).await,
                Err(BACKGROUND_MESSAGE.to_string())
            );
            assert_eq!(
                run_mutation_to_completion(&background, operation()).await,
                Err(BACKGROUND_MESSAGE.to_string())
            );
            assert!(!polled.load(Ordering::SeqCst));
        });
    }

    #[test]
    fn backgrounding_cancels_and_drops_an_in_flight_read() {
        runtime().block_on(async {
            let background = AtomicBool::new(false);
            let dropped = Arc::new(AtomicBool::new(false));
            let operation = async {
                let _guard = Dropped(dropped.clone());
                background.store(true, Ordering::SeqCst);
                std::future::pending::<Result<(), String>>().await
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                run_read_while_foreground(&background, operation),
            )
            .await
            .unwrap();
            assert_eq!(result, Err(BACKGROUND_MESSAGE.to_string()));
            assert!(dropped.load(Ordering::SeqCst));
        });
    }

    #[test]
    fn an_in_flight_write_completes_after_backgrounding() {
        runtime().block_on(async {
            let background = AtomicBool::new(false);
            let completed = AtomicBool::new(false);
            let result = run_mutation_to_completion(&background, async {
                background.store(true, Ordering::SeqCst);
                tokio::task::yield_now().await;
                completed.store(true, Ordering::SeqCst);
                Ok(7)
            })
            .await;
            assert_eq!(result, Ok(7));
            assert!(completed.load(Ordering::SeqCst));
        });
    }
}
