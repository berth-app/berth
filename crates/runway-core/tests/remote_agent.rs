//! Integration tests for remote agent communication.
//! Requires a running runway-agent at 192.168.1.222:50051

use runway_core::agent_client::AgentClient;

const REMOTE_ENDPOINT: &str = "http://192.168.1.222:50051";

#[tokio::test]
async fn test_remote_health() {
    let mut client = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect to remote agent");

    let health = client.health().await.expect("Health RPC failed");
    assert_eq!(health.status, "healthy");
    assert!(!health.version.is_empty());
    assert!(health.uptime_seconds > 0);
    println!("Health OK: v{}, uptime {}s", health.version, health.uptime_seconds);
}

#[tokio::test]
async fn test_remote_status() {
    let mut client = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect to remote agent");

    let status = client.status().await.expect("Status RPC failed");
    assert!(!status.agent_id.is_empty(), "agent_id should not be empty");
    assert_eq!(status.status, "running");
    println!(
        "Status OK: host={}, cpu={:.1}%, mem={}MB",
        status.agent_id,
        status.cpu_usage,
        status.memory_bytes / 1024 / 1024
    );
}

#[tokio::test]
async fn test_remote_execute_python() {
    let mut client = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect to remote agent");

    let code = br#"
import platform
import os
print(f"Hello from {platform.node()}!")
print(f"OS: {platform.system()} {platform.release()}")
print(f"User: {os.getenv('USER', 'unknown')}")
print(f"Python: {platform.python_version()}")
"#;

    let logs = client
        .execute("test-python", "python", "main.py", "/tmp", Some(code))
        .await
        .expect("Execute RPC failed");

    assert!(!logs.is_empty(), "Should have log output");
    let output: String = logs.iter().map(|l| format!("{}\n", l.text)).collect();
    assert!(output.contains("Hello from"), "Output should contain greeting: {output}");
    assert!(output.contains("Python:"), "Output should contain Python version: {output}");
    println!("Python execution OK:\n{output}");
}

#[tokio::test]
async fn test_remote_execute_shell() {
    let mut client = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect to remote agent");

    let code = b"#!/bin/bash\necho \"hostname: $(hostname)\"\necho \"kernel: $(uname -r)\"\necho \"uptime: $(uptime -p)\"";

    let logs = client
        .execute("test-shell", "shell", "main.sh", "/tmp", Some(code))
        .await
        .expect("Execute RPC failed");

    assert!(!logs.is_empty(), "Should have log output");
    let output: String = logs.iter().map(|l| format!("{}\n", l.text)).collect();
    assert!(output.contains("hostname:"), "Output should contain hostname: {output}");
    assert!(output.contains("kernel:"), "Output should contain kernel: {output}");
    println!("Shell execution OK:\n{output}");
}

#[tokio::test]
async fn test_remote_execute_and_stop() {
    let mut client = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect to remote agent");

    // Start a long-running process
    let code = b"#!/bin/bash\nfor i in $(seq 1 100); do echo \"tick $i\"; sleep 0.5; done";

    // Execute in background by spawning the gRPC call
    let project_id = "test-stop-me";
    let mut client2 = AgentClient::connect(REMOTE_ENDPOINT)
        .await
        .expect("Failed to connect second client");

    let exec_handle = tokio::spawn(async move {
        client2
            .execute(project_id, "shell", "main.sh", "/tmp", Some(code))
            .await
    });

    // Wait a moment for the process to start
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Stop it
    let stopped = client.stop(project_id).await.expect("Stop RPC failed");
    println!("Stop result: success={}", stopped);

    // The execute should complete (possibly with partial output)
    let result = exec_handle.await.expect("Join failed");
    match result {
        Ok(logs) => println!("Got {} log lines before stop", logs.len()),
        Err(e) => println!("Execute ended with error after stop (expected): {e}"),
    }
}
