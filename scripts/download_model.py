"""Download the all-MiniLM-L6-v2 ONNX model for local embedding.

Usage:
    uv run python scripts/download_model.py
    uv run python scripts/download_model.py --output data/models
    uv run python scripts/download_model.py --test

The model is ~23MB and produces 384-dimensional embeddings.
It runs locally via ONNX Runtime — no API calls, data never leaves your machine.
"""

import argparse
import hashlib
import sys
from pathlib import Path

# HuggingFace ONNX model files for all-MiniLM-L6-v2
MODEL_REPO = "sentence-transformers/all-MiniLM-L6-v2"
ONNX_URL = f"https://huggingface.co/{MODEL_REPO}/resolve/main/onnx/model.onnx"
TOKENIZER_URL = f"https://huggingface.co/{MODEL_REPO}/resolve/main/tokenizer.json"
TOKENIZER_CONFIG_URL = f"https://huggingface.co/{MODEL_REPO}/resolve/main/tokenizer_config.json"
SPECIAL_TOKENS_URL = f"https://huggingface.co/{MODEL_REPO}/resolve/main/special_tokens_map.json"

FILES = [
    ("model.onnx", ONNX_URL),
    ("tokenizer.json", TOKENIZER_URL),
    ("tokenizer_config.json", TOKENIZER_CONFIG_URL),
    ("special_tokens_map.json", SPECIAL_TOKENS_URL),
]


def download_file(url: str, dest: Path) -> None:
    """Download a file with progress indication."""
    import urllib.request

    print(f"  Downloading {dest.name}...", end=" ", flush=True)
    urllib.request.urlretrieve(url, dest)
    size_mb = dest.stat().st_size / (1024 * 1024)
    print(f"({size_mb:.1f} MB)")


def download_model(output_dir: Path) -> None:
    """Download all model files to the output directory."""
    output_dir.mkdir(parents=True, exist_ok=True)

    for filename, url in FILES:
        dest = output_dir / filename
        if dest.exists():
            print(f"  {filename} already exists, skipping")
            continue
        download_file(url, dest)

    print(f"\nModel downloaded to: {output_dir}")
    print(f"Set embedding_model_path = \"{output_dir / 'model.onnx'}\" in config.toml")


def run_tests() -> None:
    """Self-tests for the download module."""
    passed = 0

    # Test 1: FILES list is complete
    assert len(FILES) == 4, f"Expected 4 files, got {len(FILES)}"
    passed += 1

    # Test 2: URLs are well-formed
    for name, url in FILES:
        assert url.startswith("https://"), f"URL should be HTTPS: {url}"
        assert MODEL_REPO in url, f"URL should reference model repo: {url}"
    passed += 1

    # Test 3: File names are correct
    names = [f[0] for f in FILES]
    assert "model.onnx" in names
    assert "tokenizer.json" in names
    passed += 1

    print(f"All {passed} tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Download all-MiniLM-L6-v2 ONNX model")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("data/models"),
        help="Output directory (default: data/models)",
    )
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
    else:
        download_model(args.output)
