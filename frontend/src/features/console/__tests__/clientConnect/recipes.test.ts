/**
 * Unit tests for native client connection recipes and base URL validation.
 */

import { describe, expect, it } from 'vitest';

import type { RepositoryFormat, RepositoryMode } from '../../api/types';
import {
  buildClientRecipe,
  resolveValidatedBaseUrl,
  validateOrigin,
} from '../../clientConnect/recipes';

const ALL_FORMATS: RepositoryFormat[] = [
  'raw',
  'oci',
  'maven2',
  'npm',
  'pypi',
  'cargo',
  'nuget',
  'go',
  'helm',
  'apt',
  'rpm',
];

const ALL_MODES: RepositoryMode[] = ['hosted', 'proxy', 'group'];

const FORBIDDEN_INSECURE_PATTERNS = [
  /--insecure/i,
  /\s-k(?:\s|$)/,
  /strict-ssl\s*=\s*false/i,
  /insecure-registries/i,
  /--trusted-host/i,
  /trusted=yes/i,
  /gpgcheck=0/i,
  /sslverify=0/i,
  /GOSUMDB=off/i,
  /NODE_TLS_REJECT_UNAUTHORIZED=0/i,
];

describe('validateOrigin & resolveValidatedBaseUrl', () => {
  it('accepts a clean https or http authority', () => {
    expect(validateOrigin('https://artifacts.example.com')).toEqual({
      baseUrl: 'https://artifacts.example.com',
      authority: 'artifacts.example.com',
    });
    expect(validateOrigin('http://127.0.0.1:8080/')).toEqual({
      baseUrl: 'http://127.0.0.1:8080',
      authority: '127.0.0.1:8080',
    });
  });

  it('rejects origins with credentials, paths, queries, fragments, or invalid schemes', () => {
    expect(
      validateOrigin('https://user:pass@artifacts.example.com')
    ).toBeNull();
    expect(validateOrigin('https://artifacts.example.com/subpath')).toBeNull();
    expect(validateOrigin('https://artifacts.example.com?foo=bar')).toBeNull();
    expect(validateOrigin('https://artifacts.example.com#frag')).toBeNull();
    expect(validateOrigin('javascript:alert(1)')).toBeNull();
    expect(validateOrigin('file:///etc/passwd')).toBeNull();
    expect(validateOrigin('null')).toBeNull();
    expect(validateOrigin('')).toBeNull();
  });

  it('prefers validated configured public URL over browser origin', () => {
    expect(
      resolveValidatedBaseUrl(
        'https://registry.prod.example',
        'http://localhost:5173'
      )
    ).toEqual({
      baseUrl: 'https://registry.prod.example',
      authority: 'registry.prod.example',
      source: 'configured',
    });
  });

  it('falls back to validated browser origin when configured URL is missing or invalid', () => {
    expect(
      resolveValidatedBaseUrl(undefined, 'https://console.example.org')
    ).toEqual({
      baseUrl: 'https://console.example.org',
      authority: 'console.example.org',
      source: 'browser',
    });

    expect(
      resolveValidatedBaseUrl(
        'https://bad.example/path',
        'http://localhost:5173'
      )
    ).toEqual({
      baseUrl: 'http://localhost:5173',
      authority: 'localhost:5173',
      source: 'browser',
    });

    expect(resolveValidatedBaseUrl(undefined, 'file://local')).toBeNull();
  });
});

describe('buildClientRecipe', () => {
  it('matches format and modality for verified ecosystems', () => {
    const mavenHosted = buildClientRecipe(
      { name: 'libs-release', format: 'maven2', mode: 'hosted' },
      'https://artifacts.example.com'
    );
    expect(mavenHosted.verificationStatus).toBe('VERIFIED');
    expect(mavenHosted.clientVersion).toBe('3.9+');
    expect(mavenHosted.readOnly).toBe(false);
    expect(mavenHosted.endpointUrl).toBe(
      'https://artifacts.example.com/repository/libs-release'
    );
    expect(mavenHosted.steps.map(s => s.command).join('\n')).toContain(
      'mvn deploy'
    );

    const mavenGroup = buildClientRecipe(
      { name: 'libs-all', format: 'maven2', mode: 'group' },
      'https://artifacts.example.com'
    );
    expect(mavenGroup.readOnly).toBe(true);
    expect(mavenGroup.steps.map(s => s.command).join('\n')).toContain(
      'mvn dependency:resolve'
    );
    expect(mavenGroup.steps.map(s => s.command).join('\n')).not.toContain(
      'mvn deploy'
    );

    const npmProxy = buildClientRecipe(
      { name: 'npmjs-proxy', format: 'npm', mode: 'proxy' },
      'https://artifacts.example.com'
    );
    expect(npmProxy.verificationStatus).toBe('VERIFIED');
    expect(npmProxy.clientVersion).toBe('10+');
    expect(npmProxy.readOnly).toBe(true);
    expect(npmProxy.steps.map(s => s.command).join('\n')).toContain(
      'npm install'
    );
    expect(npmProxy.steps.map(s => s.command).join('\n')).not.toContain(
      'npm publish'
    );

    const pypiHosted = buildClientRecipe(
      { name: 'py-internal', format: 'pypi', mode: 'hosted' },
      'https://artifacts.example.com'
    );
    expect(pypiHosted.verificationStatus).toBe('VERIFIED');
    expect(pypiHosted.clientVersion).toBe('24+ / 5+');
    expect(pypiHosted.endpointUrl).toBe(
      'https://artifacts.example.com/pypi/py-internal/simple/'
    );
    expect(pypiHosted.steps.map(s => s.command).join('\n')).toContain(
      'twine upload'
    );
    expect(pypiHosted.steps.map(s => s.command).join('\n')).toContain(
      'pip install'
    );
  });

  it('marks non-accredited formats as NOT_RUN with client version', () => {
    const oci = buildClientRecipe(
      { name: 'containers', format: 'oci', mode: 'hosted' },
      'https://artifacts.example.com'
    );
    expect(oci.verificationStatus).toBe('NOT_RUN');
    expect(oci.clientVersion).toContain('26+');
  });

  it('never embeds hardcoded secrets or insecure flags across all formats and modes', () => {
    for (const format of ALL_FORMATS) {
      for (const mode of ALL_MODES) {
        const recipe = buildClientRecipe(
          { name: 'test-repo', format, mode },
          'https://artifacts.example.com'
        );
        const fullText = [
          recipe.endpointUrl,
          recipe.credentialGuidance,
          ...recipe.steps.map(s => `${s.title}\n${s.command}`),
        ].join('\n');

        for (const pattern of FORBIDDEN_INSECURE_PATTERNS) {
          expect(fullText).not.toMatch(pattern);
        }
        // Must never contain a literal bearer token
        expect(fullText).not.toMatch(/Bearer\s+[A-Za-z0-9._-]{12,}/);
      }
    }
  });
});
