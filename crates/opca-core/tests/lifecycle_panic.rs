use opca_core::lifecycle::{TaskPanic, spawn_task};

#[tokio::test]
async fn spawn_task_returns_result_on_success() {
    let result: i32 = spawn_task("t1", async { 42 }).await.unwrap();
    assert_eq!(result, 42);
}

#[tokio::test]
async fn spawn_task_catches_string_panic() {
    let err = spawn_task("t2", async {
        panic!("boom!");
    })
    .await
    .unwrap_err();

    assert_eq!(err.task_id, "t2");
    assert_eq!(err.message, "boom!");
}

#[tokio::test]
async fn spawn_task_catches_owned_string_panic() {
    let msg = String::from("dynamic failure");
    let err = spawn_task("t3", async move {
        panic!("{msg}");
    })
    .await
    .unwrap_err();

    assert_eq!(err.task_id, "t3");
    assert_eq!(err.message, "dynamic failure");
}

#[tokio::test]
async fn spawn_task_catches_non_string_panic() {
    let err = spawn_task("t4", async {
        std::panic::panic_any(42i32);
    })
    .await
    .unwrap_err();

    assert_eq!(err.task_id, "t4");
    assert_eq!(err.message, "non-string panic payload");
}

#[tokio::test]
async fn spawn_task_error_is_task_panic() {
    let result = spawn_task("t5", async {
        panic!("crashed");
    })
    .await;

    assert!(result.is_err());
    let err: TaskPanic = result.unwrap_err();
    assert!(err.message.contains("crashed"));
}

#[tokio::test]
async fn spawn_task_preserves_task_id_on_panic() {
    let err = spawn_task("unique-id-123", async {
        panic!("oops");
    })
    .await
    .unwrap_err();
    assert_eq!(err.task_id, "unique-id-123");
}

#[tokio::test]
async fn spawn_task_display_includes_task_id_and_message() {
    let err = spawn_task("task-xyz", async {
        panic!("something went wrong");
    })
    .await
    .unwrap_err();
    let display = format!("{err}");
    assert!(display.contains("task-xyz"));
    assert!(display.contains("something went wrong"));
}

#[tokio::test]
async fn spawn_task_works_with_complex_future() {
    let err = spawn_task("t6", async {
        let _ = [1, 2, 3];
        panic!("mid-computation");
    })
    .await
    .unwrap_err();
    assert_eq!(err.message, "mid-computation");
}
