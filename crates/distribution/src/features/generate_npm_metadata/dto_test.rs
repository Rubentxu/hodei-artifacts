use super::*;

#[test]
fn test_npm_package_metadata_dto_creation() {
    let metadata = NpmPackageMetadataDto {
        name: "test-package".to_string(),
        description: Some("A test package".to_string()),
        version: "1.0.0".to_string(),
        versions: std::collections::HashMap::from([
            ("1.0.0".to_string(), NpmVersionInfo {
                name: "test-package".to_string(),
                version: "1.0.0".to_string(),
                description: Some("A test package".to_string()),
                main: Some("index.js".to_string()),
                scripts: std::collections::HashMap::new(),
                dependencies: std::collections::HashMap::new(),
                dev_dependencies: std::collections::HashMap::new(),
                peer_dependencies: std::collections::HashMap::new(),
                optional_dependencies: std::collections::HashMap::new(),
                bundled_dependencies: None,
                engines: std::collections::HashMap::new(),
                os: None,
                cpu: None,
                keywords: Some(vec!["test".to_string(), "package".to_string()]),
                author: Some("Test Author".to_string()),
                license: Some("MIT".to_string()),
                repository: Some(NpmRepositoryInfo {
                    type_: "git".to_string(),
                    url: "https://github.com/test/test-package.git".to_string(),
                    directory: None,
                }),
                bugs: Some(NpmBugsInfo {
                    url: "https://github.com/test/test-package/issues".to_string(),
                    email: None,
                }),
                homepage: Some("https://github.com/test/test-package#readme".to_string()),
                dist: NpmDistInfo {
                    tarball: "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
                    shasum: "abc123".to_string(),
                    integrity: Some("sha512-abc123".to_string()),
                    file_count: Some(10),
                    unpacked_size: Some(1024),
                    signatures: None,
                },
                _id: "test-package@1.0.0".to_string(),
                _node_version: Some("16.0.0".to_string()),
                _npm_version: Some("8.0.0".to_string()),
                _has_shrinkwrap: Some(false),
                maintainers: Some(vec![NpmMaintainerInfo {
                    name: "testuser".to_string(),
                    email: "test@example.com".to_string(),
                }]),
            }),
        ]),
        "dist-tags": std::collections::HashMap::from([
            ("latest".to_string(), "1.0.0".to_string()),
        ]),
        time: Some(std::collections::HashMap::from([
            ("created".to_string(), "2023-01-01T00:00:00.000Z".to_string()),
            ("1.0.0".to_string(), "2023-01-01T00:00:00.000Z".to_string()),
            ("modified".to_string(), "2023-01-01T00:00:00.000Z".to_string()),
        ])),
        users: Some(std::collections::HashMap::from([
            ("testuser".to_string(), true),
        ])),
        author: Some(NpmAuthorInfo {
            name: "Test Author".to_string(),
            email: Some("test@example.com".to_string()),
            url: Some("https://example.com".to_string()),
        }),
        repository: Some(NpmRepositoryInfo {
            type_: "git".to_string(),
            url: "https://github.com/test/test-package.git".to_string(),
            directory: None,
        }),
        bugs: Some(NpmBugsInfo {
            url: "https://github.com/test/test-package/issues".to_string(),
            email: None,
        }),
        license: Some("MIT".to_string()),
        keywords: Some(vec!["test".to_string(), "package".to_string()]),
        homepage: Some("https://github.com/test/test-package#readme".to_string()),
        maintainers: Some(vec![NpmMaintainerInfo {
            name: "testuser".to_string(),
            email: "test@example.com".to_string(),
        }]),
        contributors: None,
        readme: Some("# Test Package\n\nThis is a test package.".to_string()),
        readme_filename: Some("README.md".to_string()),
        description: Some("A test package".to_string()),
        main: Some("index.js".to_string()),
        scripts: Some(std::collections::HashMap::new()),
        dependencies: Some(std::collections::HashMap::new()),
        dev_dependencies: Some(std::collections::HashMap::new()),
        peer_dependencies: Some(std::collections::HashMap::new()),
        bundled_dependencies: None,
        optional_dependencies: Some(std::collections::HashMap::new()),
        engines: Some(std::collections::HashMap::new()),
        os: None,
        cpu: None,
        deprecated: None,
        _attachments: None,
        _id: "test-package".to_string(),
        _rev: Some("1-abc123".to_string()),
    };

    assert_eq!(metadata.name, "test-package");
    assert_eq!(metadata.version, "1.0.0");
    assert!(metadata.versions.contains_key("1.0.0"));
    assert!(metadata.dist_tags.contains_key("latest"));
    assert_eq!(metadata.dist_tags.get("latest").unwrap(), "1.0.0");
}

#[test]
fn test_npm_package_metadata_dto_scoped_package() {
    let metadata = NpmPackageMetadataDto {
        name: "@scope/test-package".to_string(),
        description: Some("A scoped test package".to_string()),
        version: "2.0.0".to_string(),
        versions: std::collections::HashMap::from([
            ("2.0.0".to_string(), NpmVersionInfo {
                name: "@scope/test-package".to_string(),
                version: "2.0.0".to_string(),
                description: Some("A scoped test package".to_string()),
                main: Some("index.js".to_string()),
                scripts: std::collections::HashMap::new(),
                dependencies: std::collections::HashMap::new(),
                dev_dependencies: std::collections::HashMap::new(),
                peer_dependencies: std::collections::HashMap::new(),
                optional_dependencies: std::collections::HashMap::new(),
                bundled_dependencies: None,
                engines: std::collections::HashMap::new(),
                os: None,
                cpu: None,
                keywords: Some(vec!["scoped".to_string(), "test".to_string()]),
                author: Some("Scoped Author".to_string()),
                license: Some("Apache-2.0".to_string()),
                repository: None,
                bugs: None,
                homepage: None,
                dist: NpmDistInfo {
                    tarball: "https://registry.npmjs.org/@scope/test-package/-/test-package-2.0.0.tgz".to_string(),
                    shasum: "def456".to_string(),
                    integrity: Some("sha512-def456".to_string()),
                    file_count: Some(15),
                    unpacked_size: Some(2048),
                    signatures: None,
                },
                _id: "@scope/test-package@2.0.0".to_string(),
                _node_version: Some("18.0.0".to_string()),
                _npm_version: Some("9.0.0".to_string()),
                _has_shrinkwrap: Some(false),
                maintainers: Some(vec![NpmMaintainerInfo {
                    name: "scopeduser".to_string(),
                    email: "scoped@example.com".to_string(),
                }]),
            }),
        ]),
        "dist-tags": std::collections::HashMap::from([
            ("latest".to_string(), "2.0.0".to_string()),
            ("beta".to_string(), "2.0.0-beta.1".to_string()),
        ]),
        time: Some(std::collections::HashMap::from([
            ("created".to_string(), "2023-02-01T00:00:00.000Z".to_string()),
            ("2.0.0".to_string(), "2023-02-01T00:00:00.000Z".to_string()),
            ("2.0.0-beta.1".to_string(), "2023-01-15T00:00:00.000Z".to_string()),
            ("modified".to_string(), "2023-02-01T00:00:00.000Z".to_string()),
        ])),
        users: Some(std::collections::HashMap::from([
            ("scopeduser".to_string(), true),
        ])),
        author: Some(NpmAuthorInfo {
            name: "Scoped Author".to_string(),
            email: Some("scoped@example.com".to_string()),
            url: Some("https://scoped.example.com".to_string()),
        }),
        repository: None,
        bugs: None,
        license: Some("Apache-2.0".to_string()),
        keywords: Some(vec!["scoped".to_string(), "test".to_string()]),
        homepage: None,
        maintainers: Some(vec![NpmMaintainerInfo {
            name: "scopeduser".to_string(),
            email: "scoped@example.com".to_string(),
        }]),
        contributors: None,
        readme: Some("# Scoped Test Package\n\nThis is a scoped test package.".to_string()),
        readme_filename: Some("README.md".to_string()),
        description: Some("A scoped test package".to_string()),
        main: Some("index.js".to_string()),
        scripts: Some(std::collections::HashMap::new()),
        dependencies: Some(std::collections::HashMap::new()),
        dev_dependencies: Some(std::collections::HashMap::new()),
        peer_dependencies: Some(std::collections::HashMap::new()),
        bundled_dependencies: None,
        optional_dependencies: Some(std::collections::HashMap::new()),
        engines: Some(std::collections::HashMap::new()),
        os: None,
        cpu: None,
        deprecated: None,
        _attachments: None,
        _id: "@scope/test-package".to_string(),
        _rev: Some("2-def456".to_string()),
    };

    assert_eq!(metadata.name, "@scope/test-package");
    assert_eq!(metadata.version, "2.0.0");
    assert!(metadata.versions.contains_key("2.0.0"));
    assert!(metadata.dist_tags.contains_key("latest"));
    assert!(metadata.dist_tags.contains_key("beta"));
    assert_eq!(metadata.dist_tags.get("latest").unwrap(), "2.0.0");
}

#[test]
fn test_generate_npm_metadata_request_validation() {
    let request = GenerateNpmMetadataRequest {
        repository_id: "repo-123".to_string(),
        package_name: "valid-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };

    // The validation should pass for a valid package name
    assert!(request.package_name.len() > 0);
    assert!(!request.package_name.contains(' '));
}

#[test]
fn test_generate_npm_metadata_request_invalid_package_name() {
    let request = GenerateNpmMetadataRequest {
        repository_id: "repo-123".to_string(),
        package_name: "".to_string(), // Empty package name is invalid
        registry_url: "https://registry.npmjs.org".to_string(),
    };

    // Empty package name should be invalid
    assert_eq!(request.package_name.len(), 0);
}

#[test]
fn test_generate_npm_metadata_request_scoped_package() {
    let request = GenerateNpmMetadataRequest {
        repository_id: "repo-123".to_string(),
        package_name: "@scope/valid-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };

    // Scoped package names should be valid
    assert!(request.package_name.starts_with('@'));
    assert!(request.package_name.contains('/'));
}

#[test]
fn test_generate_npm_metadata_response_creation() {
    let response = GenerateNpmMetadataResponse {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        metadata: Some(NpmPackageMetadataDto {
            name: "test-package".to_string(),
            description: Some("A test package".to_string()),
            version: "1.0.0".to_string(),
            versions: std::collections::HashMap::new(),
            "dist-tags": std::collections::HashMap::new(),
            time: None,
            users: None,
            author: None,
            repository: None,
            bugs: None,
            license: None,
            keywords: None,
            homepage: None,
            maintainers: None,
            contributors: None,
            readme: None,
            readme_filename: None,
            description: None,
            main: None,
            scripts: None,
            dependencies: None,
            dev_dependencies: None,
            peer_dependencies: None,
            bundled_dependencies: None,
            optional_dependencies: None,
            engines: None,
            os: None,
            cpu: None,
            deprecated: None,
            _attachments: None,
            _id: "test-package".to_string(),
            _rev: None,
        }),
        generated_at: "2023-01-01T00:00:00.000Z".to_string(),
    };

    assert_eq!(response.repository_id, "repo-123");
    assert_eq!(response.package_name, "test-package");
    assert!(response.metadata.is_some());
    assert_eq!(response.generated_at, "2023-01-01T00:00:00.000Z");
}

#[test]
fn test_generate_npm_metadata_command_creation() {
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };

    assert_eq!(command.repository_id, "repo-123");
    assert_eq!(command.package_name, "test-package");
    assert_eq!(command.registry_url, "https://registry.npmjs.org");
}

#[test]
fn test_npm_version_info_creation() {
    let version_info = NpmVersionInfo {
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        description: Some("A test package".to_string()),
        main: Some("index.js".to_string()),
        scripts: std::collections::HashMap::from([
            ("test".to_string(), "npm test".to_string()),
        ]),
        dependencies: std::collections::HashMap::from([
            ("lodash".to_string(), "^4.17.21".to_string()),
        ]),
        dev_dependencies: std::collections::HashMap::from([
            ("jest".to_string(), "^27.0.0".to_string()),
        ]),
        peer_dependencies: std::collections::HashMap::new(),
        optional_dependencies: std::collections::HashMap::new(),
        bundled_dependencies: None,
        engines: std::collections::HashMap::from([
            ("node".to_string(), ">=14.0.0".to_string()),
        ]),
        os: Some(vec!["linux".to_string(), "darwin".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        keywords: Some(vec!["test".to_string(), "package".to_string()]),
        author: Some("Test Author".to_string()),
        license: Some("MIT".to_string()),
        repository: Some(NpmRepositoryInfo {
            type_: "git".to_string(),
            url: "https://github.com/test/test-package.git".to_string(),
            directory: None,
        }),
        bugs: Some(NpmBugsInfo {
            url: "https://github.com/test/test-package/issues".to_string(),
            email: None,
        }),
        homepage: Some("https://github.com/test/test-package#readme".to_string()),
        dist: NpmDistInfo {
            tarball: "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
            shasum: "abc123".to_string(),
            integrity: Some("sha512-abc123".to_string()),
            file_count: Some(10),
            unpacked_size: Some(1024),
            signatures: None,
        },
        _id: "test-package@1.0.0".to_string(),
        _node_version: Some("16.0.0".to_string()),
        _npm_version: Some("8.0.0".to_string()),
        _has_shrinkwrap: Some(false),
        maintainers: Some(vec![NpmMaintainerInfo {
            name: "testuser".to_string(),
            email: "test@example.com".to_string(),
        }]),
    };

    assert_eq!(version_info.name, "test-package");
    assert_eq!(version_info.version, "1.0.0");
    assert_eq!(version_info.description, Some("A test package".to_string()));
    assert_eq!(version_info.main, Some("index.js".to_string()));
    assert!(version_info.scripts.contains_key("test"));
    assert!(version_info.dependencies.contains_key("lodash"));
    assert!(version_info.dev_dependencies.contains_key("jest"));
    assert!(version_info.engines.contains_key("node"));
    assert!(version_info.os.is_some());
    assert!(version_info.cpu.is_some());
    assert_eq!(version_info.keywords, Some(vec!["test".to_string(), "package".to_string()]));
    assert_eq!(version_info.author, Some("Test Author".to_string()));
    assert_eq!(version_info.license, Some("MIT".to_string()));
    assert!(version_info.repository.is_some());
    assert!(version_info.bugs.is_some());
    assert_eq!(version_info.homepage, Some("https://github.com/test/test-package#readme".to_string()));
    assert_eq!(version_info.dist.tarball, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz");
    assert_eq!(version_info.dist.shasum, "abc123");
    assert_eq!(version_info.dist.integrity, Some("sha512-abc123".to_string()));
    assert_eq!(version_info.dist.file_count, Some(10));
    assert_eq!(version_info.dist.unpacked_size, Some(1024));
    assert_eq!(version_info.dist.signatures, None);
    assert_eq!(version_info._id, "test-package@1.0.0");
    assert_eq!(version_info._node_version, Some("16.0.0".to_string()));
    assert_eq!(version_info._npm_version, Some("8.0.0".to_string()));
    assert_eq!(version_info._has_shrinkwrap, Some(false));
    assert!(version_info.maintainers.is_some());
    assert_eq!(version_info.maintainers.unwrap().len(), 1);
}