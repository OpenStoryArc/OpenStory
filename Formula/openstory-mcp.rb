class OpenstoryMcp < Formula
  desc "MCP server for OpenStory — agent tools over stdio (optional companion)"
  homepage "https://github.com/OpenStoryArc/OpenStory"
  url "https://github.com/OpenStoryArc/OpenStory/archive/refs/tags/v0.4.0.tar.gz"
  sha256 "ca18b250ecb54b16408d63011204b192864e07f63498a12b1d95f5593c09b691"
  license "Apache-2.0"
  head "https://github.com/OpenStoryArc/OpenStory.git", branch: "master"

  # Optional companion to `openstory`: install only if you want to give an agent
  # OpenStory's MCP tools. Depends on openstory so the stack (and nats-server)
  # is present — the MCP server reads the same store and bus at runtime.
  depends_on "rust" => :build
  depends_on "openstory"

  def install
    cd "rs" do
      system "cargo", "install", *std_cargo_args(path: "mcp")
    end
  end

  def caveats
    <<~EOS
      24 tools: query your OpenStory history AND drive the dashboard
      (the agent-in-UI seam — ui_control / where_is_user / subscribe_ui_state).

      Easiest: `open-story init` offers to wire this into Claude Code for you.
      Or do it by hand (needs OpenStory running):
          claude mcp add --transport stdio openstory -- #{opt_bin}/open-story-mcp

      Query tools read OpenStory's REST API; streaming tools use its bus.
      Match these to your openstory config if you changed them (pass with
      `-e KEY=val` before the server name):
          OPENSTORY_API_URL    (default http://localhost:3002)
          OPENSTORY_API_TOKEN  (only if openstory has api_token set)
          OPENSTORY_NATS_URL   (default nats://localhost:4222)
    EOS
  end

  test do
    assert_path_exists bin/"open-story-mcp"
  end
end
