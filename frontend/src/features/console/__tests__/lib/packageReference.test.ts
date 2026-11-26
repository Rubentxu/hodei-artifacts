/**
 * Canonical encoding, formatting, and credential-free link generation for
 * package coordinates and digests.
 */

import { describe, expect, it } from 'vitest';

import {
  buildCanonicalReference,
  buildPackageDetailPath,
  buildShareablePackageUrl,
  decodePackageRouteParams,
  digestsMatch,
  encodeAssetPath,
  formatDigest,
} from '../../lib/packageReference';

describe('packageReference', () => {
  it('encodes each path segment while preserving slashes and special characters', () => {
    expect(encodeAssetPath('@scope/pkg/1.0.0+build 1/foo%2Fbar.tgz')).toBe(
      '%40scope/pkg/1.0.0%2Bbuild%201/foo%252Fbar.tgz'
    );
    expect(encodeAssetPath('/libs//nested/file name.tar.gz/')).toBe(
      'libs/nested/file%20name.tar.gz'
    );
  });

  it('builds in-app package detail paths with optional pinned digest', () => {
    const path = buildPackageDetailPath(
      'npm hosted',
      '@scope/pkg/1.0.0+build',
      'sha256:abcdef'
    );
    expect(path).toBe(
      '/console/packages/npm%20hosted/%40scope/pkg/1.0.0%2Bbuild?digest=sha256%3Aabcdef'
    );
    expect(buildPackageDetailPath('libs', 'requests/2.32.3')).toBe(
      '/console/packages/libs/requests/2.32.3'
    );
  });

  it('builds shareable URLs without credentials, tokens, or fragments', () => {
    const dirtyLocation =
      'https://alice:secret@registry.example.com:8443/console/packages/libs/old?token=tok_123&auth=bad#access_token=xyz';
    const url = buildShareablePackageUrl(
      dirtyLocation,
      'libs',
      '@scope/pkg/1.0.0+1',
      'sha256:' + 'a'.repeat(64)
    );

    expect(url).toBe(
      `https://registry.example.com:8443/console/packages/libs/%40scope/pkg/1.0.0%2B1?digest=sha256%3A${'a'.repeat(64)}`
    );
    expect(url).not.toContain('alice');
    expect(url).not.toContain('secret');
    expect(url).not.toContain('tok_123');
    expect(url).not.toContain('access_token');
    expect(url).not.toContain('hrn:');
  });

  it('builds canonical repository references without inventing HRNs', () => {
    expect(buildCanonicalReference('libs', '/@scope/pkg/1.0.0')).toBe(
      'libs/@scope/pkg/1.0.0'
    );
  });

  it('decodes route params with a single pass per segment to preserve %2F', () => {
    const encoded = buildPackageDetailPath(
      'npm hosted',
      '@scope/pkg/1.0.0+build 1/foo%2Fbar.tgz'
    );
    expect(decodePackageRouteParams(encoded)).toEqual({
      repository: 'npm hosted',
      path: '@scope/pkg/1.0.0+build 1/foo%2Fbar.tgz',
    });
  });

  it('compares expected digests against actual ContentDigest', () => {
    const actual = { algorithm: 'sha256', value: 'a'.repeat(64) };
    expect(formatDigest(actual)).toBe(`sha256:${'a'.repeat(64)}`);
    expect(digestsMatch(`sha256:${'a'.repeat(64)}`, actual)).toBe(true);
    expect(digestsMatch(`SHA256:${'A'.repeat(64)}`, actual)).toBe(true);
    expect(digestsMatch('a'.repeat(64), actual)).toBe(true);
    expect(digestsMatch(`sha256:${'b'.repeat(64)}`, actual)).toBe(false);
    expect(digestsMatch('invalid-digest', actual)).toBe(false);
  });
});
