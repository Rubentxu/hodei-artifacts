/**
 * Canonical encoding, formatting, and comparison helpers for package
 * coordinates, digests, and shareable console links.
 *
 * A repository path is a mutable coordinate: the same path may later point to
 * different bytes. A shareable link therefore pins the expected content digest
 * in `?digest=...` and never carries credentials, tokens, or userinfo.
 */

import type { ContentDigest } from '../api/types';

/** Formats a content digest as `algorithm:value`. */
export const formatDigest = (digest: ContentDigest): string =>
  `${digest.algorithm}:${digest.value}`;

/**
 * Checks whether an expected digest (from a shared link or reference) matches
 * the current artifact digest. Supports both `algorithm:value` and bare hex
 * `value` formats, case-insensitively.
 */
export const digestsMatch = (
  expected: string,
  actual: ContentDigest
): boolean => {
  const normalized = expected.trim().toLowerCase();
  if (!normalized) return true;
  if (normalized.includes(':')) {
    return normalized === formatDigest(actual).toLowerCase();
  }
  return normalized === actual.value.toLowerCase();
};

/**
 * Encodes an asset path segment by segment so slashes remain path separators
 * while special characters (`@scope`, `%2F`, spaces, `+`, etc.) are preserved
 * without escaping into parent route segments.
 */
export const encodeAssetPath = (path: string): string =>
  path
    .split('/')
    .filter(segment => segment.length > 0)
    .map(segment => encodeURIComponent(segment))
    .join('/');

const safeDecodeURIComponent = (segment: string): string => {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
};

/**
 * Decodes `repository` and `path` from a `/console/packages/:repository/*`
 * pathname using a single `decodeURIComponent` pass per segment.
 *
 * React Router's wildcard param (`*`) decodes `%252F` twice (once on the
 * matched pathname and once on the splat param), which would corrupt asset
 * paths containing literal `%2F`. Decoding each raw pathname segment once
 * preserves `@scope/pkg`, `%2F`, spaces, and `+`.
 */
export const decodePackageRouteParams = (
  pathname: string,
  fallbackRepository = '',
  fallbackPath = ''
): { repository: string; path: string } => {
  const prefix = '/console/packages/';
  if (!pathname.startsWith(prefix)) {
    return { repository: fallbackRepository, path: fallbackPath };
  }

  const rest = pathname.slice(prefix.length);
  const segments = rest.split('/').filter(segment => segment.length > 0);
  if (segments.length === 0) {
    return { repository: fallbackRepository, path: fallbackPath };
  }

  const [encodedRepo, ...encodedPathSegments] = segments;
  return {
    repository: safeDecodeURIComponent(encodedRepo),
    path: encodedPathSegments.map(safeDecodeURIComponent).join('/'),
  };
};

/**
 * Builds the canonical in-app route path for a package detail view,
 * optionally pinning an expected digest in `?digest=...`.
 */
export const buildPackageDetailPath = (
  repository: string,
  assetPath: string,
  expectedDigest?: string
): string => {
  const base = `/console/packages/${encodeURIComponent(repository)}`;
  const encodedPath = encodeAssetPath(assetPath);
  const pathname = encodedPath ? `${base}/${encodedPath}` : base;

  const trimmedDigest = expectedDigest?.trim();
  if (!trimmedDigest) return pathname;

  const params = new URLSearchParams({ digest: trimmedDigest });
  return `${pathname}?${params.toString()}`;
};

/**
 * Builds a clean, credential-free shareable URL for a package detail view
 * pinned to a specific content digest. Only the sanitized origin (scheme,
 * host, port) is used; userinfo, query parameters, and fragments from the
 * current browser location are never copied.
 */
export const buildShareablePackageUrl = (
  originOrHref: string,
  repository: string,
  assetPath: string,
  digest: string
): string => {
  const path = buildPackageDetailPath(repository, assetPath, digest);
  try {
    const parsed = new URL(originOrHref);
    if (parsed.origin && parsed.origin !== 'null') {
      return `${parsed.origin}${path}`;
    }
  } catch {
    // Fall back to the relative path when no valid origin is available.
  }
  return path;
};

/**
 * Builds the canonical repository-relative coordinate string for copying.
 * Does not invent HRNs or include credentials.
 */
export const buildCanonicalReference = (
  repository: string,
  assetPath: string
): string => {
  const cleanPath = assetPath.replace(/^\/+/, '');
  return cleanPath ? `${repository}/${cleanPath}` : repository;
};
