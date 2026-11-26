param(
  [Parameter(Mandatory=$true)]
  [string]$Domain
)

$Domain = $Domain.TrimEnd('/')
Write-Host "Dang cau hinh tat ca CLI Code voi server: $Domain" -ForegroundColor Cyan

# 1. OpenCode (V2 schema)
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.config\opencode" | Out-Null
@"
{
  "`$schema": "https://opencode.ai/config.json",
  "providers": {
    "novoid-core": {
      "name": "Novoid Core",
      "package": "@opencode/ai/providers/anthropic",
      "settings": {
        "baseURL": "$Domain/v1",
        "apiKey": "sk-novoid-proxy"
      },
      "models": {
        "novoid-claude": {
          "name": "Novoid Claude",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 200000, "output": 64000 },
          "variants": [
            { "id": "default" },
            { "id": "opus-high", "headers": { "x-novoid-tier": "opus-high" } },
            { "id": "opus-medium", "headers": { "x-novoid-tier": "opus-medium" } },
            { "id": "opus-low", "headers": { "x-novoid-tier": "opus-low" } },
            { "id": "sonnet-high", "headers": { "x-novoid-tier": "sonnet-high" } },
            { "id": "sonnet-medium", "headers": { "x-novoid-tier": "sonnet-medium" } },
            { "id": "sonnet-low", "headers": { "x-novoid-tier": "sonnet-low" } }
          ]
        },
        "novoid-default": {
          "name": "Novoid Default",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "low", "headers": { "x-novoid-tier": "low" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "high", "headers": { "x-novoid-tier": "high" } }
          ]
        },
        "novoid-flash": {
          "name": "Novoid Flash",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "high", "headers": { "x-novoid-tier": "high" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "low", "headers": { "x-novoid-tier": "low" } },
            { "id": "tiered", "headers": { "x-novoid-tier": "tiered" } },
            { "id": "thinking", "headers": { "x-novoid-tier": "thinking" } },
            { "id": "lite", "headers": { "x-novoid-tier": "lite" } }
          ]
        },
        "novoid-neon": {
          "name": "Novoid Neon",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "xhigh", "headers": { "x-novoid-tier": "xhigh" } },
            { "id": "high", "headers": { "x-novoid-tier": "high" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "low", "headers": { "x-novoid-tier": "low" } }
          ]
        },
        "novoid-hydrogen": {
          "name": "Novoid Hydrogen",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "xhigh", "headers": { "x-novoid-tier": "xhigh" } },
            { "id": "high", "headers": { "x-novoid-tier": "high" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "low", "headers": { "x-novoid-tier": "low" } },
            { "id": "tiered-xhigh", "headers": { "x-novoid-tier": "tiered-xhigh" } },
            { "id": "tiered-high", "headers": { "x-novoid-tier": "tiered-high" } },
            { "id": "sonar", "headers": { "x-novoid-tier": "sonar" } }
          ]
        },
        "novoid-carbon": {
          "name": "Novoid Carbon",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "high", "headers": { "x-novoid-tier": "high" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "low", "headers": { "x-novoid-tier": "low" } }
          ]
        },
        "novoid-argon": {
          "name": "Novoid Argon",
          "capabilities": {
            "tools": true,
            "temperature": true,
            "input": ["text", "image"],
            "output": ["text"],
            "reasoning": { "supported": true }
          },
          "limit": { "context": 1000000, "output": 65536 },
          "variants": [
            { "id": "default" },
            { "id": "high", "headers": { "x-novoid-tier": "high" } },
            { "id": "medium", "headers": { "x-novoid-tier": "medium" } },
            { "id": "low", "headers": { "x-novoid-tier": "low" } }
          ]
        }
      }
    }
  },
  "model": "novoid-core/novoid-default"
}
"@ | Set-Content -Path "$env:USERPROFILE\.config\opencode\opencode.json" -Encoding UTF8
Write-Host "[OK] OpenCode" -ForegroundColor Green

# 2. Claude Code
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude" | Out-Null
@"
{
  "env": {
    "ANTHROPIC_BASE_URL": "$Domain",
    "ANTHROPIC_API_KEY": "sk-novoid-proxy",
    "ANTHROPIC_MODEL": "novoid-default",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "novoid-claude-opus-high",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "novoid-claude-sonnet-medium",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "novoid-flash-low",
    "ANTHROPIC_SMALL_FAST_MODEL": "novoid-flash-low"
  }
}
"@ | Set-Content -Path "$env:USERPROFILE\.claude\settings.json" -Encoding UTF8
Write-Host "[OK] Claude Code" -ForegroundColor Green

# 3. Aider
@"
model: anthropic/novoid-default
weak-model: anthropic/novoid-flash-low
editor-model: anthropic/novoid-claude-opus-high
set-env:
  - ANTHROPIC_API_BASE=$Domain
  - ANTHROPIC_API_KEY=sk-novoid-proxy
"@ | Set-Content -Path "$env:USERPROFILE\.aider.conf.yml" -Encoding UTF8
Write-Host "[OK] Aider" -ForegroundColor Green

# 4. Goose
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.config\goose" | Out-Null
@"
GOOSE_PROVIDER: anthropic
GOOSE_MODEL: novoid-default
ANTHROPIC_HOST: $Domain
"@ | Set-Content -Path "$env:USERPROFILE\.config\goose\config.yaml" -Encoding UTF8
Write-Host "[OK] Goose" -ForegroundColor Green

# 5. OpenAI Codex CLI
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.codex" | Out-Null
@"
model = "novoid-default"
model_provider = "novoid-core"
model_reasoning_effort = "high"

[model_providers.novoid-core]
name = "Novoid Core"
base_url = "$Domain/v1"
env_key = "NOVOID_API_KEY"
wire_api = "chat"
"@ | Set-Content -Path "$env:USERPROFILE\.codex\config.toml" -Encoding UTF8
[System.Environment]::SetEnvironmentVariable("NOVOID_API_KEY", "sk-novoid-proxy", "User")
Write-Host "[OK] Codex CLI" -ForegroundColor Green

# 6. Crush CLI
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.config\crush" | Out-Null
@"
{
  "`$schema": "https://charm.land/crush.json",
  "providers": {
    "novoid-core": {
      "name": "Novoid Core",
      "type": "anthropic",
      "base_url": "$Domain/v1",
      "api_key": "sk-novoid-proxy",
      "models": [
        { "id": "novoid-default", "name": "Novoid Default (Auto)" },
        { "id": "novoid-default-high", "name": "Novoid Default (High)" },
        { "id": "novoid-default-medium", "name": "Novoid Default (Medium)" },
        { "id": "novoid-default-low", "name": "Novoid Default (Low)" },
        { "id": "novoid-claude", "name": "Novoid Claude (Auto)" },
        { "id": "novoid-claude-opus-high", "name": "Novoid Claude Opus (High)" },
        { "id": "novoid-claude-opus-medium", "name": "Novoid Claude Opus (Medium)" },
        { "id": "novoid-claude-opus-low", "name": "Novoid Claude Opus (Low)" },
        { "id": "novoid-claude-sonnet-high", "name": "Novoid Claude Sonnet (High)" },
        { "id": "novoid-claude-sonnet-medium", "name": "Novoid Claude Sonnet (Medium)" },
        { "id": "novoid-claude-sonnet-low", "name": "Novoid Claude Sonnet (Low)" },
        { "id": "novoid-flash", "name": "Novoid Flash (Auto)" },
        { "id": "novoid-flash-high", "name": "Novoid Flash (High)" },
        { "id": "novoid-flash-medium", "name": "Novoid Flash (Medium)" },
        { "id": "novoid-flash-low", "name": "Novoid Flash (Low)" },
        { "id": "novoid-flash-tiered", "name": "Novoid Flash (Tiered)" },
        { "id": "novoid-flash-thinking", "name": "Novoid Flash (Thinking)" },
        { "id": "novoid-flash-lite", "name": "Novoid Flash (Lite)" },
        { "id": "novoid-neon", "name": "Novoid Neon (Auto)" },
        { "id": "novoid-neon-xhigh", "name": "Novoid Neon (XHigh)" },
        { "id": "novoid-neon-high", "name": "Novoid Neon (High)" },
        { "id": "novoid-neon-medium", "name": "Novoid Neon (Medium)" },
        { "id": "novoid-neon-low", "name": "Novoid Neon (Low)" },
        { "id": "novoid-hydrogen", "name": "Novoid Hydrogen (Auto)" },
        { "id": "novoid-hydrogen-xhigh", "name": "Novoid Hydrogen (XHigh)" },
        { "id": "novoid-hydrogen-high", "name": "Novoid Hydrogen (High)" },
        { "id": "novoid-hydrogen-medium", "name": "Novoid Hydrogen (Medium)" },
        { "id": "novoid-hydrogen-low", "name": "Novoid Hydrogen (Low)" },
        { "id": "novoid-hydrogen-tiered-xhigh", "name": "Novoid Hydrogen (Tiered XHigh)" },
        { "id": "novoid-hydrogen-tiered-high", "name": "Novoid Hydrogen (Tiered High)" },
        { "id": "novoid-hydrogen-sonar", "name": "Novoid Hydrogen (Sonar)" },
        { "id": "novoid-carbon", "name": "Novoid Carbon (Auto)" },
        { "id": "novoid-carbon-high", "name": "Novoid Carbon (High)" },
        { "id": "novoid-carbon-medium", "name": "Novoid Carbon (Medium)" },
        { "id": "novoid-carbon-low", "name": "Novoid Carbon (Low)" },
        { "id": "novoid-argon", "name": "Novoid Argon (Auto)" },
        { "id": "novoid-argon-high", "name": "Novoid Argon (High)" },
        { "id": "novoid-argon-medium", "name": "Novoid Argon (Medium)" },
        { "id": "novoid-argon-low", "name": "Novoid Argon (Low)" }
      ]
    }
  }
}
"@ | Set-Content -Path "$env:USERPROFILE\.config\crush\crush.json" -Encoding UTF8
Write-Host "[OK] Crush CLI" -ForegroundColor Green

Write-Host "`nHoan tat! Khoi dong lai Terminal va chay opencode / claude / aider / goose / codex / crush." -ForegroundColor Yellow
