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

pub(crate) async fn run_read_while_foreground<T, F>(
    is_in_background: &AtomicBool,
    future: F,
) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    ensure_foreground(is_in_background)?;
    let background = wait_for_background(is_in_background);
    futures_util::pin_mut!(future, background);
    match futures_util::future::select(future, background).await {
        futures_util::future::Either::Left((result, _)) => result,
        futures_util::future::Either::Right((_, _)) => Err(BACKGROUND_MESSAGE.to_string()),
    }
}

pub(crate) async fn run_mutation_to_completion<T, F>(
    is_in_background: &AtomicBool,
    future: F,
) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    // Reject writes only before polling starts. Dropping an in-flight write
    // when the app loses focus can leave the server result unknown and make a
    // duplicate submission look safe.
    ensure_foreground(is_in_background)?;
    future.await
}
