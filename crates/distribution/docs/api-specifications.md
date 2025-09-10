# Distribution API Specifications

This document describes the API specifications for the Hodei Artifacts Distribution module, which provides support for Maven, npm, and Docker package formats.

## Overview

The Distribution module implements a unified API that supports multiple package formats:
- **Maven**: Repository format for Java artifacts (.jar, .pom, .war, etc.)
- **npm**: Package format for Node.js modules (.tgz)
- **Docker**: Container image format (manifests and blobs)

All endpoints follow RESTful principles and return appropriate HTTP status codes.

## Common Response Codes

- `200 OK`: Successful request
- `201 Created`: Resource created successfully
- `400 Bad Request`: Invalid request parameters
- `401 Unauthorized`: Authentication required
- `403 Forbidden`: Insufficient permissions
- `404 Not Found`: Resource not found
- `500 Internal Server Error`: Server error

## Maven API

### Base Path: `/maven`

Maven endpoints follow the standard Maven repository layout:
```
/maven/{groupId}/{artifactId}/{version}/{filename}
```

Where:
- `groupId`: Dot-separated group identifier (e.g., `com.example`)
- `artifactId`: Artifact identifier (e.g., `my-app`)
- `version`: Version string (e.g., `1.0.0`)
- `filename`: Artifact filename (e.g., `my-app-1.0.0.jar`)

### Endpoints

#### GET Artifact
```
GET /maven/{groupId}/{artifactId}/{version}/{filename}
```

Downloads a Maven artifact.

**Request Headers:**
- `Accept`: `application/java-archive`, `application/xml`, etc.
- `If-None-Match`: ETag for caching
- `If-Modified-Since`: Last-Modified for caching

**Response:**
- `200 OK`: Artifact content
- `304 Not Modified`: If cached version is current
- `404 Not Found`: Artifact not found

**Response Headers:**
- `Content-Type`: MIME type of the artifact
- `Content-Length`: Size in bytes
- `ETag`: Entity tag for caching
- `Last-Modified`: Last modification time

#### PUT Artifact
```
PUT /maven/{groupId}/{artifactId}/{version}/{filename}
```

Uploads a Maven artifact.

**Request Headers:**
- `Content-Type`: MIME type of the artifact
- `Content-Length`: Size in bytes

**Response:**
- `201 Created`: Artifact uploaded successfully
- `400 Bad Request`: Invalid artifact format
- `403 Forbidden`: Insufficient permissions

**Response Headers:**
- `Location`: URI of the created artifact

#### HEAD Artifact
```
HEAD /maven/{groupId}/{artifactId}/{version}/{filename}
```

Checks if a Maven artifact exists.

**Response:**
- `200 OK`: Artifact exists
- `404 Not Found`: Artifact not found

**Response Headers:**
- `Content-Type`: MIME type of the artifact
- `Content-Length`: Size in bytes
- `ETag`: Entity tag
- `Last-Modified`: Last modification time

#### GET Maven Metadata
```
GET /maven/{groupId}/{artifactId}/maven-metadata.xml
```

Retrieves Maven metadata for an artifact.

**Response:**
- `200 OK`: XML metadata
- `404 Not Found`: Metadata not found

**Response Body:**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <versioning>
    <latest>1.0.0</latest>
    <release>1.0.0</release>
    <versions>
      <version>1.0.0</version>
      <version>0.9.0</version>
    </versions>
    <lastUpdated>20231201120000</lastUpdated>
  </versioning>
</metadata>
```

## npm API

### Base Path: `/npm`

npm endpoints support both regular and scoped packages.

### Endpoints

#### GET Package
```
GET /npm/{package}
```

Downloads an npm package tarball.

**Parameters:**
- `package`: Package name (e.g., `express`, `@scope/package`)

**Response:**
- `200 OK`: Package tarball (.tgz)
- `404 Not Found`: Package not found

**Response Headers:**
- `Content-Type`: `application/octet-stream`
- `Content-Length`: Size in bytes

#### PUT Package
```
PUT /npm/{package}
```

Publishes an npm package.

**Parameters:**
- `package`: Package name (e.g., `express`, `@scope/package`)

**Request Body:** npm package metadata and tarball

**Response:**
- `201 Created`: Package published successfully
- `400 Bad Request`: Invalid package format
- `403 Forbidden`: Insufficient permissions

#### GET Package Metadata
```
GET /npm/{package}/package.json
```

Retrieves npm package metadata.

**Parameters:**
- `package`: Package name (e.g., `express`, `@scope/package`)

**Response:**
- `200 OK`: Package metadata JSON
- `404 Not Found`: Package not found

**Response Body:**
```json
{
  "name": "express",
  "dist-tags": {
    "latest": "4.18.2",
    "beta": "5.0.0-beta.1"
  },
  "versions": {
    "4.18.2": {
      "name": "express",
      "version": "4.18.2",
      "description": "Fast, unopinionated, minimalist web framework",
      "main": "index.js",
      "dist": {
        "tarball": "http://localhost:8080/npm/express/-/express-4.18.2.tgz",
        "integrity": "sha512-..."
      }
    }
  }
}
```

## Docker Registry API

### Base Path: `/v2`

Docker endpoints implement the Docker Registry HTTP API V2 specification.

### Endpoints

#### API Version Check
```
GET /v2/
```

Checks Docker Registry API version.

**Response:**
- `200 OK`: API is available

**Response Headers:**
- `Docker-Distribution-Api-Version`: `registry/2.0`

#### Catalog
```
GET /v2/_catalog
```

Lists all repositories in the registry.

**Query Parameters:**
- `n`: Maximum number of entries to return
- `last`: Last repository name from previous page

**Response:**
- `200 OK`: Repository list

**Response Body:**
```json
{
  "repositories": [
    "library/nginx",
    "library/redis",
    "library/postgres"
  ]
}
```

#### Get Manifest
```
GET /v2/{name}/manifests/{reference}
```

Retrieves a manifest by name and reference.

**Parameters:**
- `name`: Repository name (e.g., `library/nginx`)
- `reference`: Tag or digest (e.g., `latest`, `sha256:abc123...`)

**Request Headers:**
- `Accept`: Manifest media type

**Response:**
- `200 OK`: Manifest content
- `404 Not Found`: Manifest not found

**Response Headers:**
- `Content-Type`: Manifest media type
- `Docker-Content-Digest`: Manifest digest

**Response Body:**
```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
  "config": {
    "mediaType": "application/vnd.docker.container.image.v1+json",
    "size": 7023,
    "digest": "sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7"
  },
  "layers": [
    {
      "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
      "size": 32654,
      "digest": "sha256:e692418e4cbaf90ca69d05a66403747baa33ee08806650b51fab815ad7fc331f"
    }
  ]
}
```

#### Put Manifest
```
PUT /v2/{name}/manifests/{reference}
```

Uploads a manifest.

**Parameters:**
- `name`: Repository name
- `reference`: Tag or digest

**Request Headers:**
- `Content-Type`: Manifest media type

**Response:**
- `201 Created`: Manifest uploaded successfully
- `400 Bad Request`: Invalid manifest

**Response Headers:**
- `Docker-Content-Digest`: Manifest digest

#### Get Blob
```
GET /v2/{name}/blobs/{digest}
```

Downloads a blob by digest.

**Parameters:**
- `name`: Repository name
- `digest`: Blob digest (e.g., `sha256:abc123...`)

**Response:**
- `200 OK`: Blob content
- `404 Not Found`: Blob not found

**Response Headers:**
- `Content-Type`: `application/octet-stream`
- `Docker-Content-Digest`: Blob digest

#### Head Blob
```
HEAD /v2/{name}/blobs/{digest}
```

Checks if a blob exists.

**Parameters:**
- `name`: Repository name
- `digest`: Blob digest

**Response:**
- `200 OK`: Blob exists
- `404 Not Found`: Blob not found

**Response Headers:**
- `Docker-Content-Digest`: Blob digest

#### Start Blob Upload
```
POST /v2/{name}/blobs/uploads/
```

Initiates a blob upload.

**Parameters:**
- `name`: Repository name

**Response:**
- `202 Accepted`: Upload initiated

**Response Headers:**
- `Location`: Upload location URI
- `Docker-Upload-UUID`: Upload UUID
- `Range`: Supported range (e.g., `0-0`)

#### Complete Blob Upload
```
PUT /v2/{name}/blobs/uploads/{uuid}?digest={digest}
```

Completes a blob upload.

**Parameters:**
- `name`: Repository name
- `uuid`: Upload UUID
- `digest`: Expected blob digest (query parameter)

**Response:**
- `201 Created`: Blob uploaded successfully
- `400 Bad Request`: Invalid digest

**Response Headers:**
- `Docker-Content-Digest`: Blob digest

#### List Tags
```
GET /v2/{name}/tags/list
```

Lists tags for a repository.

**Parameters:**
- `name`: Repository name

**Query Parameters:**
- `n`: Maximum number of entries to return
- `last`: Last tag from previous page

**Response:**
- `200 OK`: Tag list

**Response Body:**
```json
{
  "name": "library/nginx",
  "tags": [
    "latest",
    "1.21",
    "1.21.6",
    "1.20",
    "1.19"
  ]
}
```

## Authentication and Authorization

All endpoints support authentication via standard HTTP headers. The specific authentication method depends on the repository configuration:

- **Basic Authentication**: `Authorization: Basic {credentials}`
- **Bearer Token**: `Authorization: Bearer {token}`
- **API Key**: `X-API-Key: {api_key}`

Authorization is handled through Cedar policies that define fine-grained permissions for each repository and operation.

## Error Responses

All endpoints return consistent error responses:

```json
{
  "error": {
    "code": "RESOURCE_NOT_FOUND",
    "message": "The requested resource was not found",
    "details": {
      "resource": "artifact",
      "path": "/maven/com/example/my-app/1.0.0/my-app-1.0.0.jar"
    }
  }
}
```

Common error codes:
- `RESOURCE_NOT_FOUND`: Requested resource not found
- `PERMISSION_DENIED`: Insufficient permissions
- `INVALID_REQUEST`: Invalid request parameters
- `AUTHENTICATION_FAILED`: Authentication failed
- `INTERNAL_ERROR`: Internal server error

## Rate Limiting

API endpoints may be subject to rate limiting based on:
- Authentication status (authenticated users get higher limits)
- Repository type (public vs private)
- User tier (free vs paid)

Rate limit information is returned in response headers:
- `X-RateLimit-Limit`: Maximum requests per window
- `X-RateLimit-Remaining`: Remaining requests in current window
- `X-RateLimit-Reset`: Timestamp when limit resets

## Caching

Responses include appropriate caching headers:
- `ETag`: Entity tag for conditional requests
- `Last-Modified`: Last modification timestamp
- `Cache-Control`: Caching directives

Clients should use conditional requests (`If-None-Match`, `If-Modified-Since`) to minimize bandwidth usage.

## Examples

### Maven Example
```bash
# Download a Maven artifact
curl -O http://localhost:8080/maven/com/example/my-app/1.0.0/my-app-1.0.0.jar

# Upload a Maven artifact
curl -X PUT -T my-app-1.0.0.jar \
  -H "Content-Type: application/java-archive" \
  http://localhost:8080/maven/com/example/my-app/1.0.0/my-app-1.0.0.jar

# Get Maven metadata
curl http://localhost:8080/maven/com/example/my-app/maven-metadata.xml
```

### npm Example
```bash
# Download an npm package
curl -O http://localhost:8080/npm/express

# Publish an npm package
curl -X PUT -T express-4.18.2.tgz \
  -H "Content-Type: application/octet-stream" \
  http://localhost:8080/npm/express

# Get package metadata
curl http://localhost:8080/npm/express/package.json
```

### Docker Example
```bash
# Check API version
curl -H "Accept: application/vnd.docker.distribution.api.v2+json" \
  http://localhost:8080/v2/

# List repositories
curl http://localhost:8080/v2/_catalog

# Get manifest
curl -H "Accept: application/vnd.docker.distribution.manifest.v2+json" \
  http://localhost:8080/v2/library/nginx/manifests/latest

# Upload manifest
curl -X PUT -H "Content-Type: application/vnd.docker.distribution.manifest.v2+json" \
  -d @manifest.json \
  http://localhost:8080/v2/my-app/manifests/1.0.0
```

## Integration with Package Managers

The Distribution API is designed to be compatible with standard package managers:

### Maven
Configure `settings.xml`:
```xml
<settings>
  <mirrors>
    <mirror>
      <id>hodei-artifacts</id>
      <url>http://localhost:8080/maven</url>
      <mirrorOf>*</mirrorOf>
    </mirror>
  </mirrors>
</settings>
```

### npm
Configure `.npmrc`:
```
registry=http://localhost:8080/npm
```

### Docker
Configure Docker daemon:
```json
{
  "registry-mirrors": ["http://localhost:8080"]
}
```

## Monitoring and Observability

All endpoints include comprehensive observability features:

- **Structured Logging**: Request/response logging with correlation IDs
- **Metrics**: Prometheus metrics for request counts, latencies, and errors
- **Tracing**: Distributed tracing with OpenTelemetry
- **Health Checks**: `/health` endpoint for service health monitoring

## Security Considerations

- All endpoints support HTTPS/TLS encryption
- Authentication tokens should be kept secure and rotated regularly
- Repository access is controlled through fine-grained Cedar policies
- Input validation prevents injection attacks
- Rate limiting prevents abuse
- Audit logging tracks all repository operations

## Versioning

The API uses semantic versioning. Breaking changes are introduced in major version updates, with backward compatibility maintained within major versions.

Current version: `v1.0.0`