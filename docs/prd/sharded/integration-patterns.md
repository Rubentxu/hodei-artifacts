# Integration Patterns

## 1. Event-Driven Security Automation
**Kafka events for security operations:**

```rust
// Security event producer
async fn publish_security_event(
    event: SecurityEvent,
    producer: &kafka::Producer,
) -> Result<()> {
    let event_json = serde_json::to_vec(&event)?;
    
    producer.send(
        "security-events",
        event.artifact_id().as_bytes(),
        &event_json,
    ).await?;
    
    Ok(())
}

// Common security events
enum SecurityEvent {
    VulnerabilityDetected {
        artifact_id: String,
        vulnerability: Vulnerability,
        severity: Severity,
    },
    ArtifactQuarantined {
        artifact_id: String,
        reason: QuarantineReason,
        risk_score: f32,
    },
    LicenseViolation {
        artifact_id: String,
        license: String,
        policy: LicensePolicy,
    },
    SecurityScanCompleted {
        artifact_id: String,
        scan_result: ScanResult,
        duration: Duration,
    },
}
```

## 2. External Tool Integration
**Plugin architecture for security tools:**

```rust
// Security tool plugin system
struct SecurityToolManager {
    tools: HashMap<String, Arc<dyn SecurityTool>>,
    tool_configs: HashMap<String, ToolConfiguration>,
}

impl SecurityToolManager {
    async fn run_tool_chain(&self, artifact: &Artifact) -> Result<ToolChainResult> {
        let mut results = ToolChainResult::new();
        
        for (tool_name, tool) in &self.tools {
            if let Some(config) = self.tool_configs.get(tool_name) {
                if config.enabled {
                    match tool.analyze_artifact(artifact, config).await {
                        Ok(tool_result) => {
                            results.add_tool_result(tool_name, tool_result);
                        }
                        Err(e) => {
                            results.add_tool_error(tool_name, e);
                        }
                    }
                }
            }
        }
        
        Ok(results)
    }
}
```
