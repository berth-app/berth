use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::*;
use crate::tools;

pub async fn run_stdio() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    None,
                    -32700,
                    format!("Parse error: {e}"),
                );
                let out = serde_json::to_string(&resp)?;
                stdout.write_all(out.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let response = handle_request(&request).await;
        let out = serde_json::to_string(&response)?;
        stdout.write_all(out.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

async fn handle_request(req: &JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => handle_initialize(req),
        "notifications/initialized" => {
            // Client notification, no response needed — but we still return for consistency
            JsonRpcResponse::success(req.id.clone(), json!({}))
        }
        "tools/list" => handle_tools_list(req),
        "tools/call" => handle_tools_call(req).await,
        "ping" => JsonRpcResponse::success(req.id.clone(), json!({})),
        _ => {
            // For notifications (no id), silently ignore
            if req.id.is_none() {
                JsonRpcResponse::success(None, json!({}))
            } else {
                JsonRpcResponse::error(
                    req.id.clone(),
                    -32601,
                    format!("Method not found: {}", req.method),
                )
            }
        }
    }
}

fn handle_initialize(req: &JsonRpcRequest) -> JsonRpcResponse {
    let result = InitializeResult {
        protocol_version: "2024-11-05".into(),
        capabilities: ServerCapabilities {
            tools: ToolsCapability {
                list_changed: false,
            },
        },
        server_info: ServerInfo {
            name: "berth".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
    };

    JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result).unwrap(),
    )
}

fn handle_tools_list(req: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = tools::list_tools();
    let result = ToolsListResult { tools };

    JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result).unwrap(),
    )
}

async fn handle_tools_call(req: &JsonRpcRequest) -> JsonRpcResponse {
    let params = match &req.params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                req.id.clone(),
                -32602,
                "Missing params".into(),
            );
        }
    };

    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::error(
                req.id.clone(),
                -32602,
                "Missing tool name in params".into(),
            );
        }
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let result = tools::call_tool(tool_name, &args).await;

    JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result).unwrap(),
    )
}
