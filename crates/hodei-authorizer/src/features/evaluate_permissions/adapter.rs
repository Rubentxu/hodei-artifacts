// Deprecated file: local adapters no longer needed.
// Cross-context ports (IAM / Organizations / Cache / Logger / Metrics) are injected directly
// via the EvaluatePermissions DI container using shared kernel traits.
// This module intentionally left minimal to avoid stale code and confusion.
//
// If future transport or infrastructure-specific adapters are required,
// create a new vertical slice (e.g. infrastructure/http or infrastructure/cache)
// and define them there, not here.
