# 📖 Hướng dẫn cấu hình chi tiết cho từng AI Coding CLI

Thay `<DOMAIN>` bằng địa chỉ server Novoid Proxy của bạn (ví dụ: `https://novoid-proxy.up.railway.app`).

---

## ⚡ Cài đặt nhanh 1 lệnh cho tất cả CLI

* **Linux / macOS:**
  ```bash
  chmod +x scripts/install-clis.sh
  ./scripts/install-clis.sh https://<DOMAIN>
  ```
* **Windows (PowerShell):**
  ```powershell
  .\scripts\install-clis.ps1 -Domain "https://<DOMAIN>"
  ```

---

## 🎯 Danh sách Tier hỗ trợ cho mọi CLI

Mọi harness đều có thể chọn tier theo **cú pháp `<nhom>-<tier>`** (hoặc `<nhom>#<tier>` / effort slider):

* **`novoid-claude`:** `opus-high`, `opus-medium`, `opus-low`, `sonnet-high`, `sonnet-medium`, `sonnet-low`
* **`novoid-default`:** `low`, `medium`, `high`
* **`novoid-flash`:** `high`, `medium`, `low`, `tiered`, `thinking`, `lite`
* **`novoid-neon`:** `xhigh`, `high`, `medium`, `low`
* **`novoid-hydrogen`:** `xhigh`, `high`, `medium`, `low`, `tiered-xhigh`, `tiered-high`, `sonar`
* **`novoid-carbon`:** `high`, `medium`, `low`
* **`novoid-argon`:** `high`, `medium`, `low`

---

## 1. OpenCode (V2)
* **File cấu hình:** `~/.config/opencode/opencode.json` (Linux/macOS) hoặc `%USERPROFILE%\.config\opencode\opencode.json` (Windows).
* **File mẫu:** [`configs/opencode/opencode.json`](../configs/opencode/opencode.json)
* **Chọn nhóm & tier:** Gõ `/models` để chọn 1 trong 7 nhóm (`Novoid Default`, `Novoid Claude`, `Novoid Flash`, `Novoid Neon`, `Novoid Hydrogen`, `Novoid Carbon`, `Novoid Argon`), sau đó bấm phím đổi variant (`Ctrl+T` hoặc `#<tier>`) để chọn đúng tier của nhóm đó.

---

## 2. Claude Code (Anthropic CLI)
* **File cấu hình cố định:** `~/.claude/settings.json` (Linux/macOS) hoặc `%USERPROFILE%\.claude\settings.json` (Windows).
* **File mẫu:** [`configs/claude-code/settings.json`](../configs/claude-code/settings.json)
* **Lưu ý:** `ANTHROPIC_BASE_URL` **không** có `/v1` ở cuối.
* **Cách đổi nhóm & tier:**
  * Chỉnh Effort (`low` / `medium` / `high` / `max`) trực tiếp trong Claude Code, hoặc
  * Gõ `/model novoid-claude-opus-high`, `/model novoid-hydrogen-xhigh`, `/model novoid-hydrogen-sonar`, `/model novoid-flash-thinking`, v.v.

---

## 3. Aider
* **File cấu hình:** `~/.aider.conf.yml` (Linux/macOS) hoặc `%USERPROFILE%\.aider.conf.yml` (Windows).
* **File mẫu:** [`configs/aider/.aider.conf.yml`](../configs/aider/.aider.conf.yml)
* **Khởi chạy nhanh theo từng nhóm & tier:**
  ```bash
  aider --model anthropic/novoid-default
  aider --model anthropic/novoid-claude-opus-high
  aider --model anthropic/novoid-hydrogen-xhigh
  aider --model anthropic/novoid-hydrogen-low
  ```

---

## 4. OpenAI Codex CLI
* **Novoid Proxy (`src/router.rs` / `src/server.rs`)** hỗ trợ sẵn endpoint tương thích OpenAI `/v1/chat/completions` (bao gồm tool calling & streaming) và nhận trực tiếp `reasoning_effort` + tên `<nhom>-<tier>`.
* **File cấu hình:** `~/.codex/config.toml` (Linux/macOS) hoặc `%USERPROFILE%\.codex\config.toml` (Windows).
* **File mẫu:** [`configs/codex-cli/config.toml`](../configs/codex-cli/config.toml)
* **Biến môi trường & chạy theo tier:**
  ```bash
  export NOVOID_API_KEY="sk-novoid-proxy"
  codex -m novoid-hydrogen -c model_reasoning_effort="xhigh"
  # Hoặc chọn thẳng theo tên:
  codex -m novoid-claude-opus-high
  ```

---

## 5. Goose (Block AI Agent CLI)
* **File cấu hình:** `~/.config/goose/config.yaml` (Linux/macOS) hoặc `%USERPROFILE%\.config\goose\config.yaml` (Windows).
* **File mẫu:** [`configs/goose/config.yaml`](../configs/goose/config.yaml)
* **Biến môi trường:**
  ```bash
  export GOOSE_PROVIDER="anthropic"
  export GOOSE_MODEL="novoid-hydrogen-high"
  export ANTHROPIC_HOST="https://<DOMAIN>"
  export ANTHROPIC_API_KEY="sk-novoid-proxy"
  goose session
  ```

---

## 6. Crush CLI (Charm)
* **File cấu hình:** `~/.config/crush/crush.json` (Linux/macOS) hoặc `%USERPROFILE%\.config\crush\crush.json` (Windows).
* **File mẫu:** [`configs/crush/crush.json`](../configs/crush/crush.json)
* **Khởi chạy:** Chạy `crush`, bấm `Ctrl + O` để chọn trực tiếp cả nhóm model lẫn tier (vd `Novoid Hydrogen (XHigh)`, `Novoid Claude Opus (High)`, `Novoid Hydrogen (Sonar)`...).

---

## 7. Extension trên VS Code / Cursor (Cline, Roo Code, Continue)
* **Cline / Roo Code:** Chọn API Provider là **Anthropic** → bật **Use custom base URL** → nhập `https://<DOMAIN>` → API Key `sk-novoid-proxy` → Model `novoid-hydrogen-high` (hoặc bất kỳ `<nhom>-<tier>` nào).
* **Continue:** Chọn provider **Anthropic**, `apiBase`: `https://<DOMAIN>/v1`, `model`: `novoid-claude-opus-high`.
