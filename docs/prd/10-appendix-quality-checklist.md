# 10. Appendix: Quality Checklist

Before merging any changes, verify:

- [ ] Code compiles without errors (`cargo check`)
- [ ] No warnings (`cargo clippy`)
- [ ] All tests pass (`cargo nextest run`)
- [ ] Bounded context is in its own crate
- [ ] Feature has all required files
- [ ] Ports are segregated and feature-specific
- [ ] Dependencies are injected via traits
- [ ] No direct coupling between bounded contexts
- [ ] Unit tests are implemented with mocks
- [ ] Tracing is used instead of println!
- [ ] File names follow Clean Architecture standards
- [ ] Shared kernel only contains truly shared elements
- [ ] Domain events are verified in tests
- [ ] HRN is used consistently for resource identification
- [ ] Cedar policy integration is properly implemented
- [ ] Documentation is updated with the feature
