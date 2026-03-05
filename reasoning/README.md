# Runway - Decision & Context Reasoning

Structured decision documentation using [Reasoning Formats](https://github.com/reasoning-formats/reasoning-formats) (DRF + CRF).

## Structure

```
reasoning/
  crf/                  # Context Reasoning Format (organizational context)
    runway-project.yaml # Project entity, constraints, capabilities
  drf/                  # Decision Reasoning Format (decisions with reasoning)
    001-framework-selection.yaml    # Tauri vs SwiftUI vs Electron
    002-agent-language.yaml         # Rust agent (shared with app backend)
    003-product-positioning.yaml    # Deployment control plane for AI code
    004-business-model.yaml         # Freemium, zero-infra, pricing tiers
    005-mcp-as-core.yaml            # MCP server as first-class feature
```

## How to Use

- **Before making a decision:** Check CRF context for constraints and existing facts
- **After making a decision:** Create a DRF file documenting what, why, and what was rejected
- **When context changes:** Update CRF entities (new constraints, capabilities, facts)
- **In future sessions:** Claude Code reads these files to understand prior reasoning

## Format Versions
- DRF: v0.1.0
- CRF: v0.1.0
