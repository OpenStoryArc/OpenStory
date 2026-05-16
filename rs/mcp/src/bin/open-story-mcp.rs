//! open-story-mcp — MCP server binary, stdio transport.

use anyhow::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    open_story_mcp::stdio::run(stdin, stdout).await?;
    Ok(())
}
