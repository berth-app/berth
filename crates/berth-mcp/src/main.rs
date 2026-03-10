use berth_mcp::server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MCP stdio transport — Claude Code spawns this process
    server::run_stdio().await
}
