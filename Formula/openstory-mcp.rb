class OpenstoryMcp < Formula
  desc "MCP server for OpenStory — agent tools over stdio (optional companion)"
  homepage "https://github.com/OpenStoryArc/OpenStory"
  url "https://github.com/OpenStoryArc/OpenStory/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "6bf774ac766a2f86c74dbd313df337be32291ded7877453ebd077463923f28f5"
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
      Wire OpenStory's MCP tools into your agent (needs OpenStory running):
          claude mcp add openstory stdio #{opt_bin}/open-story-mcp

      Query tools read OpenStory's REST API; streaming tools use its bus.
      Match these to your openstory config if you changed them:
          OPENSTORY_API_URL    (default http://localhost:3002)
          OPENSTORY_API_TOKEN  (only if openstory has api_token set)
          OPENSTORY_NATS_URL   (default nats://localhost:4222)
    EOS
  end

  test do
    assert_path_exists bin/"open-story-mcp"
  end
end
