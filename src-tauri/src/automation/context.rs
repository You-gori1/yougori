use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub(crate) struct OperationContext {
    pub id: String,
    pub cancellation: CancellationToken,
    pub progress: std::sync::Arc<dyn Fn(serde_json::Value) + Send + Sync>,
}
tokio::task_local! { static CURRENT: OperationContext; }
pub(crate) fn current() -> Option<OperationContext> {
    CURRENT.try_with(Clone::clone).ok()
}
pub(crate) async fn scope<T>(
    context: OperationContext,
    future: impl std::future::Future<Output = T>,
) -> T {
    CURRENT.scope(context, future).await
}
pub(crate) async fn child_scope<T>(future: impl std::future::Future<Output = T>) -> T {
    if let Some(mut context) = current() {
        context.id = format!("{}-{}", context.id, uuid::Uuid::new_v4().simple());
        context.cancellation = context.cancellation.child_token();
        scope(context, future).await
    } else {
        future.await
    }
}

pub(crate) fn progress(value: serde_json::Value) {
    if let Some(operation) = current() {
        (operation.progress)(value);
    }
}
