class Openstory < Formula
  desc "Real-time visibility into AI coding agent behavior — observe, never interfere"
  homepage "https://github.com/OpenStoryArc/OpenStory"
  url "https://github.com/OpenStoryArc/OpenStory/archive/refs/tags/v0.1.0.tar.gz"
  # sha256 is computed once v0.1.0 is tagged on master:
  #   curl -sL https://github.com/OpenStoryArc/OpenStory/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "Apache-2.0"
  head "https://github.com/OpenStoryArc/OpenStory.git", branch: "master"

  depends_on "node" => :build
  depends_on "rust" => :build
  depends_on "nats-server"

  def install
    # Build the React dashboard.
    cd "ui" do
      system "npm", "ci"
      system "npm", "run", "build"
    end

    # Build and install the Rust CLI (binary lands at #{bin}/open-story).
    cd "rs" do
      system "cargo", "install", *std_cargo_args(path: "cli")
    end

    # Ship UI static assets next to the binary; --static-dir points here.
    (pkgshare/"static").install Dir["ui/dist/*"]

    # Create the per-machine data directory so first boot has somewhere to write.
    (var/"openstory").mkpath
    (var/"log").mkpath
  end

  service do
    # `--manage-nats` makes serve launch and supervise a JetStream nats-server
    # itself (Homebrew's nats-server runs without JetStream), so a single
    # `brew services start openstory` brings up the whole stack. `--nats-bin`
    # passes the resolved keg path because launchd's PATH is minimal.
    run [
      opt_bin/"open-story", "serve",
      "--static-dir", "#{HOMEBREW_PREFIX}/share/openstory/static",
      "--data-dir", "#{HOMEBREW_PREFIX}/var/openstory",
      "--manage-nats",
      "--nats-bin", "#{Formula["nats-server"].opt_bin}/nats-server"
    ]
    keep_alive true
    log_path var/"log/openstory.log"
    error_log_path var/"log/openstory.error.log"
  end

  def caveats
    <<~EOS
      OpenStory brings up everything it needs — it launches a JetStream
      NATS server automatically, so there's no separate step.

      Guided setup (history window, watch dir, port — then starts the service):
          open-story init --data-dir #{var}/openstory

      Or just start the service:
          brew services start openstory

      Open the dashboard:
          http://localhost:3002

      Data dir:       #{var}/openstory
      UI assets:      #{pkgshare}/static
      Watched dir:    ~/.claude/projects/  (Claude Code default)
    EOS
  end

  test do
    # `open-story --help` should mention the `serve` subcommand.
    assert_match "serve", shell_output("#{bin}/open-story --help")
  end
end
