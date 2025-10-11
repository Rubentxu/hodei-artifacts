#!/bin/bash

echo "🚀 Setting up fast testing tools..."

# Instalar cargo-nextest
if ! command -v cargo-nextest &> /dev/null; then
    echo "Installing cargo-nextest..."
    cargo install cargo-nextest
else
    echo "✅ cargo-nextest already installed"
fi

# Instalar sccache
if ! command -v sccache &> /dev/null; then
    echo "Installing sccache..."
    cargo install sccache
else
    echo "✅ sccache already installed"
fi

# Instalar cargo-watch
if ! command -v cargo-watch &> /dev/null; then
    echo "Installing cargo-watch..."
    cargo install cargo-watch
else
    echo "✅ cargo-watch already installed"
fi

# Instalar cargo-cache
if ! command -v cargo-cache &> /dev/null; then
    echo "Installing cargo-cache..."
    cargo install cargo-cache
else
    echo "✅ cargo-cache already installed"
fi

# Configurar sccache
echo ""
echo "📝 Configuring sccache..."
echo "Add to your ~/.bashrc or ~/.zshrc:"
echo "export RUSTC_WRAPPER=sccache"
echo "export SCCACHE_DIR=\$HOME/.cache/sccache"
echo "export SCCACHE_CACHE_SIZE=\"10G\""

# Verificar mold (solo Linux)
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    if ! command -v mold &> /dev/null; then
        echo ""
        echo "⚠️  mold linker not found (optional but recommended)"
        echo "Install with: sudo apt install mold  # or pacman -S mold"
    else
        echo "✅ mold linker installed"
    fi
fi

echo ""
echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "1. Add sccache env vars to your shell config"
echo "2. Restart your shell or run: source ~/.bashrc"
echo "3. Run: sccache --start-server"
